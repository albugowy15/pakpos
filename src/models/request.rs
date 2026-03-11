use uuid::Uuid;

use crate::models::{KeyValueField, method::Method};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Request {
    pub id: String,
    pub title: String,
    pub method: Method,
    pub url: Option<String>,
    pub query_params: Vec<KeyValueField>,
    pub headers: Vec<KeyValueField>,
    pub body: Option<String>,
}

impl Request {
    pub fn new(title: String) -> Self {
        let id = Uuid::new_v4().to_string();
        Self {
            id,
            title,
            headers: vec![KeyValueField {
                id: Uuid::new_v4().to_string(),
                key: Some("Content-Type".to_owned()),
                value: Some("application/json".to_owned()),
            }],
            ..Default::default()
        }
    }
}

pub trait FindRequest {
    fn find_by_id(&self, id: &str) -> Option<&Request>;
    fn find_by_id_mut(&mut self, id: &str) -> Option<&mut Request>;
}

impl FindRequest for Vec<Request> {
    fn find_by_id(&self, id: &str) -> Option<&Request> {
        self.iter().find(|req| req.id == id)
    }

    fn find_by_id_mut(&mut self, id: &str) -> Option<&mut Request> {
        self.iter_mut().find(|req| req.id == id)
    }
}
