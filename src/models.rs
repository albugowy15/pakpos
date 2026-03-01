use std::fmt;

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum EditorTab {
    #[default]
    Params,
    Authorization,
    Headers,
    Body,
}

pub const EDITOR_TABS: [EditorTab; 4] = [
    EditorTab::Params,
    EditorTab::Authorization,
    EditorTab::Headers,
    EditorTab::Body,
];

impl fmt::Display for EditorTab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Params => "Params",
            Self::Body => "Body",
            Self::Authorization => "Authorization",
            Self::Headers => "Headers",
        })
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HTTPMethod {
    #[default]
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
}

pub const HTTP_METHODS: [HTTPMethod; 6] = [
    HTTPMethod::Get,
    HTTPMethod::Post,
    HTTPMethod::Put,
    HTTPMethod::Delete,
    HTTPMethod::Patch,
    HTTPMethod::Head,
];

impl fmt::Display for HTTPMethod {
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
