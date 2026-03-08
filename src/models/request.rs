use uuid::Uuid;

use crate::models::{KeyValueField, Method};

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub id: String,
    pub title: String,
    pub method: Method,
    pub url: Option<String>,
    pub query_params: Vec<KeyValueField>,
    pub header: Vec<KeyValueField>,
    pub body: Option<String>,
}

impl Request {
    pub fn new(title: String) -> Self {
        let id = Uuid::new_v4().to_string();
        Self {
            id,
            title,
            ..Default::default()
        }
    }
}

pub trait FindRequest {
    fn find_by_id(&self, id: String) -> Option<&Request>;
}

impl FindRequest for Vec<Request> {
    fn find_by_id(&self, id: String) -> Option<&Request> {
        if self.is_empty() {
            return None;
        }
        for req in self {
            if req.id == id {
                return Some(req);
            } else {
                return None;
            }
        }
        None
    }
}
