use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub title: String,
}

impl fmt::Display for Workspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.title)
    }
}

impl Workspace {
    pub fn new(title: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_creation() {
        let ws = Workspace::new(String::from("Test Workspace"));
        assert_eq!(ws.title, "Test Workspace");
        assert!(!ws.id.is_empty());
    }

    #[test]
    fn test_workspace_display() {
        let ws = Workspace::new(String::from("My Workspace"));
        assert_eq!(format!("{}", ws), "My Workspace");
    }
}
