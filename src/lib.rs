pub mod app;
pub mod models;
pub mod net;
pub mod storage;
pub mod ui;

#[derive(Debug, Clone)]
pub enum Error {
    Api,
    Serde,
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        dbg!(value);
        Self::Api
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        dbg!(value);
        Self::Serde
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_debug() {
        assert_eq!(format!("{:?}", Error::Api), "Api");
        assert_eq!(format!("{:?}", Error::Serde), "Serde");
    }

    #[test]
    fn test_error_from_serde_json() {
        let result: Result<serde_json::Value, _> = serde_json::from_str("{ invalid }");
        let err = result.unwrap_err();
        let error: Error = err.into();
        assert!(matches!(error, Error::Serde));
    }
}
