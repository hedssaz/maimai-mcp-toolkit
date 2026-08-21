use maimai_core::scoring::ScoringError;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum AdapterError {
    #[error("{0}")]
    Input(String),

    #[error(transparent)]
    Scoring(#[from] ScoringError),

    #[error("failed to serialize scoring result: {0}")]
    Json(#[from] serde_json::Error),
}

impl AdapterError {
    pub(super) fn input(message: impl Into<String>) -> Self {
        Self::Input(message.into())
    }
}
