use std::{fmt, path::PathBuf};

use uuid::Uuid;

#[derive(Debug)]
pub enum StorageError {
    MissingDataDirectory,
    RequestNotFound(Uuid),
    InvalidData {
        message: String,
    },
    Filesystem {
        path: PathBuf,
        source: std::io::Error,
    },
    Database(rusqlite::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDataDirectory => formatter.write_str(
                "Could not resolve the user data directory because HOME is unavailable.",
            ),
            Self::RequestNotFound(id) => write!(formatter, "Saved request {id} was not found."),
            Self::InvalidData { message } => {
                write!(
                    formatter,
                    "The collection database contains invalid data: {message}."
                )
            }
            Self::Filesystem { path, source } => {
                write!(formatter, "Could not access {}: {source}", path.display())
            }
            Self::Database(error) => write!(formatter, "Collection database error: {error}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Filesystem { source, .. } => Some(source),
            Self::Database(source) => Some(source),
            _ => None,
        }
    }
}
