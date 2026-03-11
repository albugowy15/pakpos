use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
