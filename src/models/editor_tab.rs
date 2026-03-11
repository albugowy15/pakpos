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
