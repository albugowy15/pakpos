use std::{fmt, path::PathBuf, str::FromStr, time::Duration};

use reqwest::{
    Method, Url,
    header::{HeaderName, HeaderValue},
};
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

/// A request as edited and persisted by Pakpos.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<HeaderRow>,
    pub body: RequestBody,
}

impl Request {
    pub fn validated(mut self) -> Result<Self, ValidationError> {
        self.check(true)?;
        let start = self.url.len() - self.url.trim_start().len();
        let end = self.url.trim_end().len();
        self.url.truncate(end);
        self.url.drain(..start);
        self.headers
            .retain(|header| header.enabled && !header.is_blank());
        if let RequestBody::Multipart(fields) = &mut self.body {
            fields.retain(|field| field.enabled && !field.is_blank());
        }
        Ok(self)
    }

    pub(crate) fn check(&self, validate_files: bool) -> Result<(), ValidationError> {
        let entered_url = self.url.trim();
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

        for (index, header) in self.headers.iter().enumerate() {
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
            HeaderValue::from_str(&header.value)
                .map_err(|_| ValidationError::InvalidHeaderValue { row })?;
        }

        match &self.body {
            RequestBody::None => {}
            RequestBody::Json(text) => {
                if text.trim().is_empty() {
                    return Err(ValidationError::EmptyJsonBody);
                }
                validate_json(text).map_err(|error| ValidationError::InvalidJson {
                    line: error.line(),
                    column: error.column(),
                    message: error.to_string(),
                })?;
            }
            RequestBody::Multipart(fields) => {
                if self.headers.iter().any(|header| {
                    header.enabled
                        && !header.is_blank()
                        && header.name.eq_ignore_ascii_case("content-type")
                }) {
                    return Err(ValidationError::MultipartContentType);
                }

                for (index, field) in fields.iter().enumerate() {
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
                }
            }
        }
        Ok(())
    }
}

// Validate with serde_json's normal scalar and recursion checks, but discard
// values as they are visited instead of allocating a Value tree.
pub(crate) fn validate_json(text: &str) -> Result<(), serde_json::Error> {
    serde_json::from_str::<ValidJson>(text).map(|_| ())
}

struct ValidJson;

impl<'de> Deserialize<'de> for ValidJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Self)
    }
}

impl<'de> serde::de::Visitor<'de> for ValidJson {
    type Value = Self;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self, E> {
        Ok(Self)
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Self, E> {
        Ok(Self)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self, A::Error> {
        while seq.next_element::<Self>()?.is_some() {}
        Ok(Self)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self, A::Error> {
        while map.next_entry::<Self, Self>()?.is_some() {}
        Ok(Self)
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
    InvalidHeaderValue {
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
            Self::InvalidHeaderValue { row } => {
                write!(formatter, "Header row {row} has an invalid value.")
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

    fn request(url: &str) -> Request {
        Request {
            url: url.to_owned(),
            ..Request::default()
        }
    }

    #[test]
    fn streaming_json_validation_matches_value_validation() {
        for text in [
            r#"{"a":[1,true,null,{"escaped\u0020key":"a\nb"}],"a":2}"#,
            "1e9999",
            r#""\uD800""#,
            r#"{"\uD800":0}"#,
            "[1,]",
            "[0] trailing",
            "{} {}",
            "",
            "null",
            "-1.25e3",
            "18446744073709551616",
        ] {
            assert_eq!(
                validate_json(text).is_ok(),
                serde_json::from_str::<serde_json::Value>(text).is_ok(),
                "{text}"
            );
        }
        let deep = format!("{}0{}", "[".repeat(140), "]".repeat(140));
        assert!(validate_json(&deep).is_err());
    }

    #[test]
    fn validation_reuses_url_headers_and_multipart_buffers() {
        let request = Request {
            url: "  https://example.com/path?a=1&b=2  ".into(),
            headers: vec![HeaderRow::default(), HeaderRow::enabled("X-Test", "yes")],
            body: RequestBody::Multipart(vec![
                MultipartField::text("", ""),
                MultipartField::text("field", "value"),
            ]),
            ..Request::default()
        };
        let url_ptr = request.url.as_ptr();
        let headers_ptr = request.headers.as_ptr();
        let RequestBody::Multipart(fields) = &request.body else {
            unreachable!()
        };
        let fields_ptr = fields.as_ptr();
        let request = request.validated().unwrap();
        assert_eq!(url_ptr, request.url.as_ptr());
        assert_eq!(headers_ptr, request.headers.as_ptr());
        assert_eq!(request.url, "https://example.com/path?a=1&b=2");
        let RequestBody::Multipart(fields) = request.body else {
            unreachable!()
        };
        assert_eq!(fields_ptr, fields.as_ptr());
        assert_eq!(fields.len(), 1);
    }

    #[test]
    fn accepts_http_url_without_rewriting_query_order() {
        let request = request("https://example.com/x?b=2&a=1")
            .validated()
            .unwrap();
        assert_eq!(request.url, "https://example.com/x?b=2&a=1");
    }

    #[test]
    fn rejects_non_http_scheme() {
        let error = request("file:///tmp/example").validated().unwrap_err();
        assert_eq!(error, ValidationError::UnsupportedScheme("file".to_owned()));
    }

    #[test]
    fn keeps_enabled_duplicate_headers_in_order() {
        let mut value = request("http://localhost:3000");
        value.headers = vec![
            HeaderRow::enabled("X-Tag", "first"),
            HeaderRow {
                enabled: false,
                name: "X-Skip".to_owned(),
                value: "no".to_owned(),
            },
            HeaderRow::enabled("X-Tag", "second"),
        ];
        let request = value.validated().unwrap();
        assert_eq!(request.headers.len(), 2);
        assert_eq!(request.headers[0].value, "first");
        assert_eq!(request.headers[1].value, "second");
    }

    #[test]
    fn rejects_invalid_header_values() {
        let mut value = request("https://example.com");
        value.headers = vec![HeaderRow::enabled("X-Test", "bad\0value")];
        assert_eq!(
            value.validated().unwrap_err(),
            ValidationError::InvalidHeaderValue { row: 1 }
        );
    }

    #[test]
    fn validates_any_nonempty_json_value() {
        for json in ["null", "42", "[]", "{\"ok\":true}"] {
            let mut value = request("https://example.com");
            value.body = RequestBody::Json(json.to_owned());
            value.validated().unwrap();
        }
    }

    #[test]
    fn reports_json_error_location() {
        let mut value = request("https://example.com");
        value.body = RequestBody::Json("{\n  nope\n}".to_owned());
        let error = value.validated().unwrap_err();
        assert!(matches!(
            error,
            ValidationError::InvalidJson { line: 2, .. }
        ));
    }

    #[test]
    fn validates_and_filters_multipart_fields() {
        let path = temporary_file(b"file bytes");
        let mut value = request("https://example.com/upload");
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

        let request = value.validated().unwrap();
        let RequestBody::Multipart(fields) = request.body else {
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
        let mut value = request("https://example.com/upload");
        value.headers = vec![HeaderRow::enabled("Content-Type", "multipart/form-data")];
        value.body = RequestBody::Multipart(vec![MultipartField::text("name", "Pakpos")]);

        assert_eq!(
            value.validated().unwrap_err(),
            ValidationError::MultipartContentType
        );
    }

    #[test]
    fn rejects_missing_or_unreadable_multipart_files() {
        let mut missing = request("https://example.com/upload");
        missing.body = RequestBody::Multipart(vec![MultipartField::file("asset", "")]);
        assert!(matches!(
            missing.validated().unwrap_err(),
            ValidationError::MissingMultipartFile { row: 1 }
        ));

        let mut unreadable = request("https://example.com/upload");
        unreadable.body = RequestBody::Multipart(vec![MultipartField::file(
            "asset",
            "/missing/pakpos-test-file",
        )]);
        assert!(matches!(
            unreadable.validated().unwrap_err(),
            ValidationError::UnreadableMultipartFile { row: 1, .. }
        ));
    }
}
