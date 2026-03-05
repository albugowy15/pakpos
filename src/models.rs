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
