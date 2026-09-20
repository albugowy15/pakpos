use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use pakpos::{
    collections::CollectionRequest,
    models::{HeaderRow, MultipartField, MultipartValue, Request, RequestBody},
    net,
    postman::{self, ImportedCollection},
};
use serde_json::Value;
use tokio::sync::oneshot;

const FIXTURE_NAME: &str = "representative-v2.1.postman_collection.json";
const SCHEMA_NAME: &str = "postman-collection-v2.1.0-schema.json";

fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/postman")
}

fn read_fixture(name: &str) -> String {
    fs::read_to_string(fixture_directory().join(name)).expect("fixture should be readable")
}

fn import_fixture() -> ImportedCollection {
    postman::import_collection(&read_fixture(FIXTURE_NAME), &fixture_directory())
        .expect("representative fixture should import")
}

fn assert_same_supported_requests(left: &[CollectionRequest], right: &[CollectionRequest]) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right) {
        assert_eq!(left.node.name, right.node.name);
        assert_eq!(left.node.position, right.node.position);
        assert_eq!(left.request, right.request);
    }
}

#[test]
fn representative_fixture_imports_the_supported_subset() {
    let imported = import_fixture();

    assert_eq!(imported.summary.name, "Pakpos interoperability");
    assert_eq!(
        imported
            .requests
            .iter()
            .map(|request| request.node.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Send JSON",
            "Upload multipart",
            "Unsupported body becomes empty",
        ]
    );

    let json = &imported.requests[0].request;
    assert_eq!(
        json.url,
        "http://127.0.0.1:1/echo/json?tag=one&tag=two%20words"
    );
    assert_eq!(
        json.headers,
        [
            HeaderRow::enabled("X-Trace", "first"),
            HeaderRow::enabled("X-Trace", "second"),
            HeaderRow {
                enabled: false,
                name: "X-Trace".to_owned(),
                value: "disabled".to_owned(),
            },
            HeaderRow::enabled("Authorization", "Bearer manual-token"),
        ]
    );
    assert_eq!(
        json.body,
        RequestBody::Json(r#"{"message":"hello","count":2}"#.to_owned())
    );

    let RequestBody::Multipart(fields) = &imported.requests[1].request.body else {
        panic!("multipart request should retain form-data fields");
    };
    assert_eq!(fields[0], MultipartField::text("label", "one"));
    assert_eq!(fields[1], MultipartField::text("label", "two"));
    assert_eq!(
        fields[2],
        MultipartField::file("asset", fixture_directory().join("assets/upload.txt"))
    );
    assert_eq!(
        fields[3],
        MultipartField {
            enabled: false,
            name: "hidden".to_owned(),
            value: MultipartValue::Text("ignored".to_owned()),
        }
    );
    assert_eq!(imported.requests[2].request.body, RequestBody::None);
}

#[test]
fn exported_fixture_validates_against_the_official_v21_schema_and_round_trips() {
    let imported = import_fixture();
    let exported =
        postman::export_collection(&imported.summary, &imported.requests, &fixture_directory())
            .expect("fixture should export");
    let document: Value = serde_json::from_str(&exported).expect("export should be JSON");
    let schema: Value =
        serde_json::from_str(&read_fixture(SCHEMA_NAME)).expect("schema fixture should be JSON");
    let validator = jsonschema::draft4::options()
        .offline()
        .build(&schema)
        .expect("official Postman schema should compile");
    let errors = validator
        .iter_errors(&document)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "export did not satisfy the Postman v2.1 schema:\n{}",
        errors.join("\n")
    );
    let mut invalid_document = document.clone();
    invalid_document
        .as_object_mut()
        .expect("export root should be an object")
        .remove("info");
    assert!(!validator.is_valid(&invalid_document));

    let round_tripped = postman::import_collection(&exported, &fixture_directory())
        .expect("export should import again");
    assert_same_supported_requests(&imported.requests, &round_tripped.requests);
}

fn read_http_request(mut stream: &TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout should be configurable");
    let mut request = Vec::new();
    let mut expected_length = None;
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream
            .read(&mut buffer)
            .expect("request should be readable");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if expected_length.is_none()
            && let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
        {
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let body_length = headers.lines().find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|value| value.parse::<usize>().ok())
            });
            expected_length = body_length.map(|length| (header_end + 4, length));
        }
        if expected_length.is_some_and(|(body_start, length)| request.len() >= body_start + length)
        {
            break;
        }
    }
    request
}

fn start_server(
    request_count: usize,
) -> (
    SocketAddr,
    mpsc::Receiver<Vec<u8>>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have an address");
    let (sender, receiver) = mpsc::channel();
    let server = std::thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().expect("request should connect");
            let request = read_http_request(&stream);
            sender.send(request).expect("request should be captured");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("response should be writable");
        }
    });
    (address, receiver, server)
}

fn target_server(mut request: Request, address: SocketAddr) -> Request {
    request.url = request
        .url
        .replacen("http://127.0.0.1:1", &format!("http://{address}"), 1);
    request
}

async fn send(request: Request) {
    let (_cancel_sender, cancel_receiver) = oneshot::channel();
    let response = net::execute(request, cancel_receiver)
        .await
        .expect("fixture request should succeed");
    assert_eq!(response.status, 200);
}

fn text_request(request: &[u8]) -> String {
    String::from_utf8_lossy(request).into_owned()
}

fn assert_json_request(request: &[u8]) {
    let request = text_request(request);
    assert!(request.starts_with("POST /echo/json?tag=one&tag=two%20words HTTP/1.1\r\n"));
    assert_eq!(request.matches("x-trace:").count(), 2);
    assert!(request.contains("x-trace: first\r\n"));
    assert!(request.contains("x-trace: second\r\n"));
    assert!(!request.contains("disabled"));
    assert!(request.contains("authorization: Bearer manual-token\r\n"));
    assert!(request.contains("content-type: application/json\r\n"));
    assert!(request.ends_with(r#"{"message":"hello","count":2}"#));
}

fn assert_multipart_request(request: &[u8]) {
    let request = text_request(request);
    assert!(request.starts_with("POST /echo/upload HTTP/1.1\r\n"));
    assert!(request.contains("content-type: multipart/form-data; boundary="));
    assert_eq!(request.matches("name=\"label\"").count(), 2);
    assert!(request.contains("\r\n\r\none\r\n"));
    assert!(request.contains("\r\n\r\ntwo\r\n"));
    assert!(request.contains("name=\"asset\""));
    assert!(request.contains("filename=\"upload.txt\""));
    assert!(request.contains("exact multipart fixture bytes\n"));
    assert!(!request.contains("name=\"hidden\""));
}

#[tokio::test]
async fn imported_and_round_tripped_requests_behave_equivalently_on_the_wire() {
    let imported = import_fixture();
    let exported =
        postman::export_collection(&imported.summary, &imported.requests, &fixture_directory())
            .expect("fixture should export");
    let round_tripped = postman::import_collection(&exported, &fixture_directory())
        .expect("export should import again");
    let (address, requests, server) = start_server(4);

    send(target_server(
        imported.requests[0].request.as_ref().clone(),
        address,
    ))
    .await;
    send(target_server(
        round_tripped.requests[0].request.as_ref().clone(),
        address,
    ))
    .await;
    send(target_server(
        imported.requests[1].request.as_ref().clone(),
        address,
    ))
    .await;
    send(target_server(
        round_tripped.requests[1].request.as_ref().clone(),
        address,
    ))
    .await;

    let captured = (0..4)
        .map(|_| {
            requests
                .recv_timeout(Duration::from_secs(5))
                .expect("server should capture every request")
        })
        .collect::<Vec<_>>();
    server.join().expect("server should stop cleanly");

    assert_json_request(&captured[0]);
    assert_json_request(&captured[1]);
    assert_multipart_request(&captured[2]);
    assert_multipart_request(&captured[3]);
}
