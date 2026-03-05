use std::fmt;

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum EditorTab {
    #[default]
    Params,
    Headers,
    Body,
}

pub const EDITOR_TABS: [EditorTab; 3] = [EditorTab::Params, EditorTab::Headers, EditorTab::Body];

impl fmt::Display for EditorTab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Params => "Params",
            Self::Body => "Body",
            Self::Headers => "Headers",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    QueryParam,
    Header,
}

#[derive(Debug, Clone)]
pub struct KeyValueField {
    pub id: String,
    pub key: Option<String>,
    pub value: Option<String>,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
}

pub const METHODS: [Method; 6] = [
    Method::Get,
    Method::Post,
    Method::Put,
    Method::Delete,
    Method::Patch,
    Method::Head,
];

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_tab_display() {
        assert_eq!(EditorTab::Params.to_string(), "Params");
        assert_eq!(EditorTab::Headers.to_string(), "Headers");
        assert_eq!(EditorTab::Body.to_string(), "Body");
    }

    #[test]
    fn test_method_display() {
        assert_eq!(Method::Get.to_string(), "GET");
        assert_eq!(Method::Post.to_string(), "POST");
        assert_eq!(Method::Put.to_string(), "PUT");
        assert_eq!(Method::Delete.to_string(), "DELETE");
        assert_eq!(Method::Patch.to_string(), "PATCH");
        assert_eq!(Method::Head.to_string(), "HEAD");
    }

    #[test]
    fn test_key_value_field_creation() {
        let field = KeyValueField {
            id: "123".to_owned(),
            key: Some("Content-Type".to_owned()),
            value: Some("application/json".to_owned()),
        };
        assert_eq!(field.id, "123");
        assert_eq!(field.key.as_deref(), Some("Content-Type"));
        assert_eq!(field.value.as_deref(), Some("application/json"));
    }
}
