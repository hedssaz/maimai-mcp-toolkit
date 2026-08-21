use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("failed to prepare storage path {path}: {source}")]
    PreparePath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("SQLite operation failed: {0}")]
    Sql(#[from] sqlx::Error),

    #[error("stored JSON in {field} is invalid: {source}")]
    StoredJson {
        field: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to encode JSON for {field}: {source}")]
    EncodeJson {
        field: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("stored {field} value is invalid: {value}")]
    InvalidStoredValue { field: &'static str, value: String },

    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },

    #[error("failed to format current UTC timestamp: {0}")]
    Timestamp(#[from] time::error::Format),

    #[error("stored timestamp in {field} is invalid: {source}")]
    ParseTimestamp {
        field: &'static str,
        #[source]
        source: time::error::Parse,
    },
}
