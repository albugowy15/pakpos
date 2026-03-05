use crate::{Error, models::Method};
use serde::Serialize;

pub struct RequestTask {
    pub method: Method,
    pub url: String,
    pub body: String,
    pub query_params: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
}

impl RequestTask {
    pub fn new(method: Method, url: String) -> Self {
        Self {
            method,
            url,
            body: String::new(),
            query_params: Vec::new(),
            headers: Vec::new(),
        }
    }

    pub fn body(mut self, body: String) -> Self {
        self.body = body;
        self
    }

    pub fn query_params(mut self, params: Vec<(String, String)>) -> Self {
        self.query_params = params;
        self
    }

    pub fn headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    pub async fn execute(self) -> Result<String, Error> {
        let client = reqwest::Client::new();
        let mut req = match self.method {
            Method::Get => client.get(&self.url),
            Method::Post => client.post(&self.url),
            Method::Put => client.put(&self.url),
            Method::Delete => client.delete(&self.url),
            Method::Patch => client.patch(&self.url),
            Method::Head => client.head(&self.url),
        };

        if !self.query_params.is_empty() {
            req = req.query(&self.query_params);
        }

        for (key, val) in self.headers {
            req = req.header(key, val);
        }

        if !self.body.is_empty() {
            req = req.body(self.body.clone());
        }

        let result = req.send().await?;
        let response = result.text().await?;

        let parsed: serde_json::Value = match serde_json::from_str(&response) {
            Ok(val) => val,
            Err(_) => return Ok(response),
        };

        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        parsed.serialize(&mut ser)?;
        Ok(String::from_utf8(buf).map_err(|_| Error::SerdeError)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_task_builder() {
        let task = RequestTask::new(Method::Post, "https://example.com".to_string())
            .body("hello".to_string())
            .query_params(vec![("a".to_owned(), "b".to_owned())])
            .headers(vec![("c".to_owned(), "d".to_owned())]);

        assert_eq!(task.method, Method::Post);
        assert_eq!(task.url, "https://example.com");
        assert_eq!(task.body, "hello");
        assert_eq!(task.query_params, vec![("a".to_owned(), "b".to_owned())]);
        assert_eq!(task.headers, vec![("c".to_owned(), "d".to_owned())]);
    }
}
