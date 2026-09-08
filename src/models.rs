use std::{fmt, path::PathBuf, str::FromStr, time::Duration};

use reqwest::{Method, Url, header::HeaderName};
use serde::{Deserialize, Serialize};

pub const RESPONSE_PREVIEW_LIMIT: usize = 5 * 1024 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl HttpMethod {
    pub const ALL: [Self; 6] = [
        Self::Get,
        Self::Post,
        Self::Put,
        Self::Patch,
        Self::Delete,
        Self::Head,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for HttpMethod {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "GET" => Ok(Self::Get),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "PATCH" => Ok(Self::Patch),
            "DELETE" => Ok(Self::Delete),
            "HEAD" => Ok(Self::Head),
            _ => Err(ValidationError::UnsupportedMethod(value.to_owned())),
        }
    }
}

impl From<HttpMethod> for Method {
    fn from(value: HttpMethod) -> Self {
        match value {
            HttpMethod::Get => Self::GET,
            HttpMethod::Post => Self::POST,
            HttpMethod::Put => Self::PUT,
            HttpMethod::Patch => Self::PATCH,
            HttpMethod::Delete => Self::DELETE,
            HttpMethod::Head => Self::HEAD,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderRow {
    pub enabled: bool,
    pub name: String,
    pub value: String,
}

impl HeaderRow {
    pub fn enabled(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            enabled: true,
            name: name.into(),
            value: value.into(),
        }
    }

    fn is_blank(&self) -> bool {
        self.name.trim().is_empty() && self.value.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "content", rename_all = "snake_case")]
pub enum RequestBody {
    #[default]
    None,
    Json(String),
    Multipart(Vec<MultipartField>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum MultipartValue {
    Text(String),
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultipartField {
    pub enabled: bool,
    pub name: String,
    pub value: MultipartValue,
}

impl MultipartField {
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            enabled: true,
            name: name.into(),
            value: MultipartValue::Text(value.into()),
        }
    }

    pub fn file(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            enabled: true,
            name: name.into(),
            value: MultipartValue::File(path.into()),
        }
    }

    fn is_blank(&self) -> bool {
        self.name.trim().is_empty()
            && match &self.value {
                MultipartValue::Text(value) => value.is_empty(),
                MultipartValue::File(path) => path.as_os_str().is_empty(),
            }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestDraft {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<HeaderRow>,
    pub body: RequestBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSnapshot {
    pub method: HttpMethod,
    pub url: Url,
    pub headers: Vec<HeaderRow>,
    pub body: RequestBody,
}

impl TryFrom<RequestDraft> for RequestSnapshot {
    type Error = ValidationError;

    fn try_from(draft: RequestDraft) -> Result<Self, Self::Error> {
        Self::from_draft(draft, true)
    }
}

impl RequestSnapshot {
    pub(crate) fn try_from_import(draft: RequestDraft) -> Result<Self, ValidationError> {
        Self::from_draft(draft, false)
    }

    fn from_draft(draft: RequestDraft, validate_files: bool) -> Result<Self, ValidationError> {
        let entered_url = draft.url.trim();
        if entered_url.is_empty() {
            return Err(ValidationError::MissingUrl);
        }

        let url = Url::parse(entered_url).map_err(|error| ValidationError::InvalidUrl {
            reason: error.to_string(),
        })?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ValidationError::UnsupportedScheme(url.scheme().to_owned()));
        }
        if url.host().is_none() {
            return Err(ValidationError::InvalidUrl {
                reason: "the URL must include a host".to_owned(),
            });
        }

        let mut headers = Vec::with_capacity(draft.headers.len());
        for (index, header) in draft.headers.into_iter().enumerate() {
            if header.is_blank() || !header.enabled {
                continue;
            }
            let row = index + 1;
            if header.name.trim().is_empty() {
                return Err(ValidationError::MissingHeaderName { row });
            }
            if header.name.contains(['\r', '\n']) || header.value.contains(['\r', '\n']) {
                return Err(ValidationError::HeaderContainsNewline { row });
            }
            HeaderName::from_bytes(header.name.as_bytes())
                .map_err(|_| ValidationError::InvalidHeaderName { row })?;
            headers.push(header);
        }

        let body = match draft.body {
            RequestBody::None => RequestBody::None,
            RequestBody::Json(text) => {
                if text.trim().is_empty() {
                    return Err(ValidationError::EmptyJsonBody);
                }
                serde_json::from_str::<serde_json::Value>(&text).map_err(|error| {
                    ValidationError::InvalidJson {
                        line: error.line(),
                        column: error.column(),
                        message: error.to_string(),
                    }
                })?;
                RequestBody::Json(text)
            }
            RequestBody::Multipart(fields) => {
                if headers
                    .iter()
                    .any(|header| header.name.eq_ignore_ascii_case("content-type"))
                {
                    return Err(ValidationError::MultipartContentType);
                }

                let mut validated = Vec::with_capacity(fields.len());
                for (index, field) in fields.into_iter().enumerate() {
                    if !field.enabled || field.is_blank() {
                        continue;
                    }
                    let row = index + 1;
                    if field.name.trim().is_empty() {
                        return Err(ValidationError::MissingMultipartName { row });
                    }
                    if field.name.contains(['\r', '\n']) {
                        return Err(ValidationError::MultipartNameContainsNewline { row });
                    }
                    if let MultipartValue::File(path) = &field.value {
                        if path.as_os_str().is_empty() {
                            return Err(ValidationError::MissingMultipartFile { row });
                        }
                        if validate_files {
                            let metadata = std::fs::metadata(path).map_err(|error| {
                                ValidationError::UnreadableMultipartFile {
                                    row,
                                    path: path.clone(),
                                    reason: error.to_string(),
                                }
                            })?;
                            if !metadata.is_file() {
                                return Err(ValidationError::UnreadableMultipartFile {
                                    row,
                                    path: path.clone(),
                                    reason: "the selected path is not a regular file".to_owned(),
                                });
                            }
                            std::fs::File::open(path).map_err(|error| {
                                ValidationError::UnreadableMultipartFile {
                                    row,
                                    path: path.clone(),
                                    reason: error.to_string(),
                                }
                            })?;
                        }
                    }
                    validated.push(field);
                }
                RequestBody::Multipart(validated)
            }
        };

        Ok(Self {
            method: draft.method,
            url,
            headers,
            body,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    MissingUrl,
    InvalidUrl {
        reason: String,
    },
    UnsupportedScheme(String),
    UnsupportedMethod(String),
    MissingHeaderName {
        row: usize,
    },
    InvalidHeaderName {
        row: usize,
    },
    HeaderContainsNewline {
        row: usize,
    },
    EmptyJsonBody,
    InvalidJson {
        line: usize,
        column: usize,
        message: String,
    },
    MissingMultipartName {
        row: usize,
    },
    MultipartNameContainsNewline {
        row: usize,
    },
    MissingMultipartFile {
        row: usize,
    },
    UnreadableMultipartFile {
        row: usize,
        path: PathBuf,
        reason: String,
    },
    MultipartContentType,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingUrl => formatter.write_str("Enter an HTTP or HTTPS URL."),
            Self::InvalidUrl { reason } => write!(formatter, "The URL is invalid: {reason}."),
            Self::UnsupportedScheme(scheme) => write!(
                formatter,
                "The URL scheme ‘{scheme}’ is not supported. Use http:// or https://."
            ),
            Self::UnsupportedMethod(method) => {
                write!(formatter, "The request method ‘{method}’ is not supported.")
            }
            Self::MissingHeaderName { row } => write!(formatter, "Header row {row} needs a name."),
            Self::InvalidHeaderName { row } => {
                write!(formatter, "Header row {row} has an invalid name.")
            }
            Self::HeaderContainsNewline { row } => {
                write!(formatter, "Header row {row} contains a line break.")
            }
            Self::EmptyJsonBody => formatter.write_str("Enter a JSON body or select None."),
            Self::InvalidJson {
                line,
                column,
                message,
            } => write!(
                formatter,
                "The JSON body is invalid at line {line}, column {column}: {message}"
            ),
            Self::MissingMultipartName { row } => {
                write!(formatter, "Multipart row {row} needs a field name.")
            }
            Self::MultipartNameContainsNewline { row } => {
                write!(formatter, "Multipart row {row} has a line break in its name.")
            }
            Self::MissingMultipartFile { row } => {
                write!(formatter, "Multipart row {row} needs a file.")
            }
            Self::UnreadableMultipartFile { row, path, reason } => write!(
                formatter,
                "The file in multipart row {row} cannot be read ({}): {reason}.",
                path.display()
            ),
            Self::MultipartContentType => formatter.write_str(
                "Remove the manual Content-Type header. Pakpos generates the multipart boundary automatically.",
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_file(contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("pakpos-model-test-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn draft(url: &str) -> RequestDraft {
        RequestDraft {
            url: url.to_owned(),
            ..RequestDraft::default()
        }
    }

    #[test]
    fn accepts_http_url_without_rewriting_query_order() {
        let snapshot = RequestSnapshot::try_from(draft("https://example.com/x?b=2&a=1")).unwrap();
        assert_eq!(snapshot.url.as_str(), "https://example.com/x?b=2&a=1");
    }

    #[test]
    fn rejects_non_http_scheme() {
        let error = RequestSnapshot::try_from(draft("file:///tmp/example")).unwrap_err();
        assert_eq!(error, ValidationError::UnsupportedScheme("file".to_owned()));
    }

    #[test]
    fn keeps_enabled_duplicate_headers_in_order() {
        let mut value = draft("http://localhost:3000");
        value.headers = vec![
            HeaderRow::enabled("X-Tag", "first"),
            HeaderRow {
                enabled: false,
                name: "X-Skip".to_owned(),
                value: "no".to_owned(),
            },
            HeaderRow::enabled("X-Tag", "second"),
        ];
        let snapshot = RequestSnapshot::try_from(value).unwrap();
        assert_eq!(snapshot.headers.len(), 2);
        assert_eq!(snapshot.headers[0].value, "first");
        assert_eq!(snapshot.headers[1].value, "second");
    }

    #[test]
    fn validates_any_nonempty_json_value() {
        for json in ["null", "42", "[]", "{\"ok\":true}"] {
            let mut value = draft("https://example.com");
            value.body = RequestBody::Json(json.to_owned());
            RequestSnapshot::try_from(value).unwrap();
        }
    }

    #[test]
    fn reports_json_error_location() {
        let mut value = draft("https://example.com");
        value.body = RequestBody::Json("{\n  nope\n}".to_owned());
        let error = RequestSnapshot::try_from(value).unwrap_err();
        assert!(matches!(
            error,
            ValidationError::InvalidJson { line: 2, .. }
        ));
    }

    #[test]
    fn validates_and_filters_multipart_fields() {
        let path = temporary_file(b"file bytes");
        let mut value = draft("https://example.com/upload");
        value.body = RequestBody::Multipart(vec![
            MultipartField::text("tag", "one"),
            MultipartField {
                enabled: false,
                name: "ignored".to_owned(),
                value: MultipartValue::File("/missing/disabled-file".into()),
            },
            MultipartField::text("tag", ""),
            MultipartField::file("asset", &path),
            MultipartField::text("", ""),
        ]);

        let snapshot = RequestSnapshot::try_from(value).unwrap();
        let RequestBody::Multipart(fields) = snapshot.body else {
            panic!("expected multipart body");
        };
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].name, "tag");
        assert_eq!(fields[1].name, "tag");
        assert_eq!(fields[2].name, "asset");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_manual_multipart_content_type() {
        let mut value = draft("https://example.com/upload");
        value.headers = vec![HeaderRow::enabled("Content-Type", "multipart/form-data")];
        value.body = RequestBody::Multipart(vec![MultipartField::text("name", "Pakpos")]);

        assert_eq!(
            RequestSnapshot::try_from(value).unwrap_err(),
            ValidationError::MultipartContentType
        );
    }

    #[test]
    fn rejects_missing_or_unreadable_multipart_files() {
        let mut missing = draft("https://example.com/upload");
        missing.body = RequestBody::Multipart(vec![MultipartField::file("asset", "")]);
        assert!(matches!(
            RequestSnapshot::try_from(missing).unwrap_err(),
            ValidationError::MissingMultipartFile { row: 1 }
        ));

        let mut unreadable = draft("https://example.com/upload");
        unreadable.body = RequestBody::Multipart(vec![MultipartField::file(
            "asset",
            "/missing/pakpos-test-file",
        )]);
        assert!(matches!(
            RequestSnapshot::try_from(unreadable).unwrap_err(),
            ValidationError::UnreadableMultipartFile { row: 1, .. }
        ));
    }
}
