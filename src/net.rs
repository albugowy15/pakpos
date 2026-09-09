use std::{fmt, path::PathBuf, time::Instant};

use reqwest::{
    Client,
    header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
    multipart::{Form, Part},
    redirect::Policy,
};
use tokio::sync::oneshot;

use crate::models::{MultipartValue, REQUEST_TIMEOUT, Request, RequestBody, ValidationError};
use crate::response::{ResponseBodyCollector, system_download_directory};
pub use crate::response::{ResponseData, ResponseHeader};

#[derive(Debug)]
pub enum RequestError {
    Validation(ValidationError),
    Cancelled,
    Timeout,
    InvalidHeader { name: String, reason: String },
    File { path: String, reason: String },
    Transport(reqwest::Error),
}

impl fmt::Display for RequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::Cancelled => formatter.write_str("Request cancelled."),
            Self::Timeout => formatter.write_str("The request timed out after 30 seconds."),
            Self::InvalidHeader { name, reason } => {
                write!(formatter, "Header ‘{name}’ could not be sent: {reason}.")
            }
            Self::File { path, reason } => {
                write!(
                    formatter,
                    "Could not read multipart file ‘{path}’: {reason}."
                )
            }
            Self::Transport(error) if error.is_connect() => {
                write!(formatter, "Could not connect to the server: {error}")
            }
            Self::Transport(error) => write!(formatter, "The request failed: {error}"),
        }
    }
}

impl std::error::Error for RequestError {}

pub async fn execute(
    request: Request,
    cancel: oneshot::Receiver<()>,
) -> Result<ResponseData, RequestError> {
    execute_with_download_directory(request, cancel, system_download_directory()).await
}

pub async fn execute_with_download_directory(
    request: Request,
    cancel: oneshot::Receiver<()>,
    download_directory: Result<PathBuf, String>,
) -> Result<ResponseData, RequestError> {
    tokio::select! {
        _ = cancel => Err(RequestError::Cancelled),
        result = tokio::time::timeout(REQUEST_TIMEOUT, execute_inner(request, download_directory)) => {
            result.map_err(|_| RequestError::Timeout)?
        }
    }
}

async fn execute_inner(
    request: Request,
    download_directory: Result<PathBuf, String>,
) -> Result<ResponseData, RequestError> {
    let request = request.validated().map_err(RequestError::Validation)?;
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .map_err(RequestError::Transport)?;
    let mut headers = HeaderMap::new();
    let has_content_type = request
        .headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(CONTENT_TYPE.as_str()));

    for header in request.headers {
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|error| {
            RequestError::InvalidHeader {
                name: header.name.clone(),
                reason: error.to_string(),
            }
        })?;
        let value =
            HeaderValue::from_str(&header.value).map_err(|error| RequestError::InvalidHeader {
                name: header.name.clone(),
                reason: error.to_string(),
            })?;
        headers.append(name, value);
    }

    let mut outbound = client
        .request(request.method.into(), request.url)
        .headers(headers);
    match request.body {
        RequestBody::None => {}
        RequestBody::Json(body) => {
            if !has_content_type {
                outbound = outbound.header(CONTENT_TYPE, "application/json");
            }
            outbound = outbound.body(body);
        }
        RequestBody::Multipart(fields) => {
            let mut form = Form::new();
            for field in fields {
                form = match field.value {
                    MultipartValue::Text(value) => form.text(field.name, value),
                    MultipartValue::File(path) => {
                        let display_path = path.display().to_string();
                        let part = Part::file(&path)
                            .await
                            .map_err(|error| RequestError::File {
                                path: display_path,
                                reason: error.to_string(),
                            })?;
                        form.part(field.name, part)
                    }
                };
            }
            outbound = outbound.multipart(form);
        }
    }

    let started = Instant::now();
    let mut response = outbound.send().await.map_err(RequestError::Transport)?;
    let status = response.status();
    let response_headers = response
        .headers()
        .iter()
        .map(|(name, value)| ResponseHeader {
            name: name.as_str().to_owned(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
    let content_disposition = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
    let mut body = ResponseBodyCollector::new(
        content_type.as_deref(),
        content_disposition.as_deref(),
        response.url(),
        download_directory,
    );

    let mut body_size = 0_u64;
    while let Some(chunk) = response.chunk().await.map_err(RequestError::Transport)? {
        body_size = body_size.saturating_add(chunk.len() as u64);
        body.push(&chunk);
    }

    Ok(ResponseData {
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or("Response").to_owned(),
        elapsed: started.elapsed(),
        body_size,
        headers: response_headers,
        body: body.finish(),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };

    use super::*;
    use crate::models::{HeaderRow, HttpMethod, Request, RequestBody};
    use crate::response::{ResponseBody, ResponseTextKind};

    fn response(preview: Vec<u8>, content_type: Option<&str>) -> ResponseData {
        let kind = if content_type
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"))
        {
            ResponseTextKind::Json
        } else {
            ResponseTextKind::Plain
        };
        ResponseData {
            status: 200,
            reason: "OK".to_owned(),
            elapsed: std::time::Duration::ZERO,
            body_size: preview.len() as u64,
            headers: Vec::new(),
            body: if preview.is_empty() {
                ResponseBody::Empty
            } else {
                ResponseBody::Text {
                    text: String::from_utf8_lossy(&preview).into_owned(),
                    kind,
                    truncated: false,
                    saved_path: None,
                    notices: Vec::new(),
                }
            },
        }
    }

    #[test]
    fn formats_response_summary() {
        let mut value = response(Vec::new(), None);
        value.status = 404;
        value.reason = "Not Found".to_owned();
        value.elapsed = std::time::Duration::from_millis(12);
        value.body_size = 1536;
        assert_eq!(value.summary(), "404 Not Found  •  12 ms  •  1.5 KiB");
    }

    #[test]
    fn pretty_prints_json_responses_case_insensitively() {
        let value = response(
            br#"{"a":1}"#.to_vec(),
            Some("Application/JSON; charset=utf-8"),
        );
        assert_eq!(value.display_body(), "{\n  \"a\": 1\n}");
    }

    #[tokio::test]
    async fn validates_the_request_at_the_execution_boundary() {
        let (_cancel_sender, cancel_receiver) = oneshot::channel();
        let error = execute(Request::default(), cancel_receiver)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            RequestError::Validation(ValidationError::MissingUrl)
        ));
    }

    #[tokio::test]
    async fn sends_json_and_preserves_duplicate_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_sender, request_receiver) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.ends_with(br#"{"sent":true}"#) {
                    break;
                }
            }
            request_sender.send(request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nX-Reply: one\r\nX-Reply: two\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
                )
                .unwrap();
        });

        let request = Request {
            method: HttpMethod::Post,
            url: format!("http://{address}/items?b=2&a=1"),
            headers: vec![
                HeaderRow::enabled("X-Tag", "first"),
                HeaderRow::enabled("X-Tag", "second"),
            ],
            body: RequestBody::Json("{\"sent\":true}".to_owned()),
        };
        let (_cancel_sender, cancel_receiver) = oneshot::channel();
        let response = execute(request, cancel_receiver).await.unwrap();
        let request = String::from_utf8(request_receiver.recv().unwrap()).unwrap();
        server.join().unwrap();

        assert!(request.starts_with("POST /items?b=2&a=1 HTTP/1.1\r\n"));
        assert!(request.contains("x-tag: first\r\n"));
        assert!(request.contains("x-tag: second\r\n"));
        assert!(request.contains("content-type: application/json\r\n"));
        assert!(request.ends_with("{\"sent\":true}"));
        assert_eq!(response.status, 201);
        assert_eq!(
            response
                .headers
                .iter()
                .filter(|header| header.name == "x-reply")
                .count(),
            2
        );
        assert_eq!(response.display_body(), "{\n  \"ok\": true\n}");
    }

    #[tokio::test]
    async fn streams_multipart_text_and_file_fields() {
        let file_path = std::env::temp_dir().join(format!(
            "pakpos-multipart-test-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&file_path, b"streamed file bytes").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_sender, request_receiver) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut expected_length = None;
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if expected_length.is_none()
                    && let Some(header_end) =
                        request.windows(4).position(|part| part == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers.lines().find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    });
                    expected_length = content_length.map(|length| (header_end + 4, length));
                }
                if expected_length
                    .is_some_and(|(body_start, length)| request.len() >= body_start + length)
                {
                    break;
                }
            }
            request_sender.send(request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
        });

        let request = Request {
            method: HttpMethod::Post,
            url: format!("http://{address}/upload"),
            headers: Vec::new(),
            body: RequestBody::Multipart(vec![
                crate::models::MultipartField::text("tag", "one"),
                crate::models::MultipartField::text("tag", "two"),
                crate::models::MultipartField::file("asset", &file_path),
            ]),
        };
        let (_cancel_sender, cancel_receiver) = oneshot::channel();
        let response = execute(request, cancel_receiver).await.unwrap();
        let request = String::from_utf8(request_receiver.recv().unwrap()).unwrap();
        server.join().unwrap();
        std::fs::remove_file(file_path).unwrap();

        assert!(request.starts_with("POST /upload HTTP/1.1\r\n"));
        assert!(request.contains("content-type: multipart/form-data; boundary="));
        assert_eq!(request.matches("name=\"tag\"").count(), 2);
        assert!(request.contains("\r\n\r\none\r\n"));
        assert!(request.contains("\r\n\r\ntwo\r\n"));
        assert!(request.contains("name=\"asset\""));
        assert!(request.contains("filename=\""));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("content-type: text/plain")
        );
        assert!(request.contains("\r\n\r\nstreamed file bytes\r\n"));
        assert_eq!(response.status, 200);
        assert_eq!(response.body_size, 0);
    }

    #[tokio::test]
    async fn streams_binary_response_to_the_selected_download_directory() {
        let download_directory =
            std::env::temp_dir().join(format!("pakpos-net-download-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&download_directory).unwrap();
        let response_bytes = b"\x89PNG\r\n\x1a\nexact binary bytes";

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"../../image.png\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_bytes.len()
            )
            .unwrap();
            stream.write_all(response_bytes).unwrap();
        });

        let request = Request {
            url: format!("http://{address}/download"),
            ..Request::default()
        };
        let (_cancel_sender, cancel_receiver) = oneshot::channel();
        let response = execute_with_download_directory(
            request,
            cancel_receiver,
            Ok(download_directory.clone()),
        )
        .await
        .unwrap();
        server.join().unwrap();

        let crate::response::ResponseBody::Downloaded { path } = response.body else {
            panic!("expected a downloaded response");
        };
        assert_eq!(path.parent(), Some(download_directory.as_path()));
        assert_eq!(path.file_name().unwrap(), "image.png");
        assert_eq!(std::fs::read(&path).unwrap(), response_bytes);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(download_directory).unwrap();
    }
}
