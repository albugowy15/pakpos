use std::{fmt, str::FromStr, time::Duration};

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

        if let RequestBody::Json(text) = &draft.body {
            if text.trim().is_empty() {
                return Err(ValidationError::EmptyJsonBody);
            }
            serde_json::from_str::<serde_json::Value>(text).map_err(|error| {
                ValidationError::InvalidJson {
                    line: error.line(),
                    column: error.column(),
                    message: error.to_string(),
                }
            })?;
        }

        Ok(Self {
            method: draft.method,
            url,
            headers,
            body: draft.body,
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
        }
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

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
}
