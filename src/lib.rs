pub mod app;
pub mod models;
pub mod net;
pub mod ui;

#[derive(Debug, Clone)]
pub enum Error {
    APIError,
    SerdeError,
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        dbg!(value);
        Self::APIError
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        dbg!(value);
        Self::SerdeError
    }
}
