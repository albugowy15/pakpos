use std::{fmt, time::Instant};

use reqwest::{
    Client,
    header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
    redirect::Policy,
};
use tokio::sync::oneshot;

use crate::models::{REQUEST_TIMEOUT, RESPONSE_PREVIEW_LIMIT, RequestBody, RequestSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct ResponseData {
    pub status: u16,
    pub reason: String,
    pub elapsed: std::time::Duration,
    pub body_size: u64,
    pub headers: Vec<ResponseHeader>,
    pub preview: Vec<u8>,
    pub preview_truncated: bool,
    pub content_type: Option<String>,
}

impl ResponseData {
    pub fn summary(&self) -> String {
        format!(
            "{} {}  •  {} ms  •  {}",
            self.status,
            self.reason,
            self.elapsed.as_millis(),
            format_byte_count(self.body_size)
        )
    }

    pub fn display_body(&self) -> String {
        if self.preview.is_empty() {
            return "This response has no body.".to_owned();
        }

        let decoded = String::from_utf8_lossy(&self.preview);
        let is_json = self
            .content_type
            .as_deref()
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .is_some_and(|media_type| {
                media_type.eq_ignore_ascii_case("application/json")
                    || media_type.to_ascii_lowercase().ends_with("+json")
            });

        let mut text = if is_json && !self.preview_truncated {
            serde_json::from_str::<serde_json::Value>(&decoded)
                .and_then(|value| serde_json::to_string_pretty(&value))
                .unwrap_or_else(|_| decoded.into_owned())
        } else {
            decoded.into_owned()
        };

        if self.preview_truncated {
            text.push_str("\n\n— Preview stopped at 5 MiB —");
        }
        text
    }

    pub fn display_headers(&self) -> String {
        self.headers
            .iter()
            .map(|header| format!("{}: {}", header.name, header.value))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug)]
pub enum RequestError {
    Cancelled,
    Timeout,
    InvalidHeader { name: String, reason: String },
    Transport(reqwest::Error),
}

impl fmt::Display for RequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Request cancelled."),
            Self::Timeout => formatter.write_str("The request timed out after 30 seconds."),
            Self::InvalidHeader { name, reason } => {
                write!(formatter, "Header ‘{name}’ could not be sent: {reason}.")
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
    snapshot: RequestSnapshot,
    cancel: oneshot::Receiver<()>,
) -> Result<ResponseData, RequestError> {
    tokio::select! {
        _ = cancel => Err(RequestError::Cancelled),
        result = tokio::time::timeout(REQUEST_TIMEOUT, execute_inner(snapshot)) => {
            result.map_err(|_| RequestError::Timeout)?
        }
    }
}

async fn execute_inner(snapshot: RequestSnapshot) -> Result<ResponseData, RequestError> {
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .map_err(RequestError::Transport)?;
    let mut headers = HeaderMap::new();
    let has_content_type = snapshot
        .headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(CONTENT_TYPE.as_str()));

    for header in snapshot.headers {
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

    let mut request = client
        .request(snapshot.method.into(), snapshot.url)
        .headers(headers);
    if let RequestBody::Json(body) = snapshot.body {
        if !has_content_type {
            request = request.header(CONTENT_TYPE, "application/json");
        }
        request = request.body(body);
    }

    let started = Instant::now();
    let mut response = request.send().await.map_err(RequestError::Transport)?;
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

    let mut body_size = 0_u64;
    let mut preview = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(RequestError::Transport)? {
        body_size = body_size.saturating_add(chunk.len() as u64);
        let remaining = RESPONSE_PREVIEW_LIMIT.saturating_sub(preview.len());
        if remaining > 0 {
            preview.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
    }

    Ok(ResponseData {
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or("Response").to_owned(),
        elapsed: started.elapsed(),
        body_size,
        headers: response_headers,
        preview_truncated: body_size > preview.len() as u64,
        preview,
        content_type,
    })
}

fn format_byte_count(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };

    use super::*;
    use crate::models::{HeaderRow, HttpMethod, RequestBody, RequestDraft, RequestSnapshot};

    fn response(preview: Vec<u8>, content_type: Option<&str>) -> ResponseData {
        ResponseData {
            status: 200,
            reason: "OK".to_owned(),
            elapsed: std::time::Duration::ZERO,
            body_size: preview.len() as u64,
            headers: Vec::new(),
            preview,
            preview_truncated: false,
            content_type: content_type.map(str::to_owned),
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

        let draft = RequestDraft {
            method: HttpMethod::Post,
            url: format!("http://{address}/items?b=2&a=1"),
            headers: vec![
                HeaderRow::enabled("X-Tag", "first"),
                HeaderRow::enabled("X-Tag", "second"),
            ],
            body: RequestBody::Json("{\"sent\":true}".to_owned()),
        };
        let snapshot = RequestSnapshot::try_from(draft).unwrap();
        let (_cancel_sender, cancel_receiver) = oneshot::channel();
        let response = execute(snapshot, cancel_receiver).await.unwrap();
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
}
