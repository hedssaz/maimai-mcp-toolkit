use std::{error::Error, fmt};

use maimai_core::RatingError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreErrorCode {
    UnsupportedSource,
    ChartNotFound,
    AmbiguousChart,
    InvalidRecord,
    Catalog,
    Rating,
}

#[derive(Debug)]
pub struct ScoreError {
    code: ScoreErrorCode,
    message: String,
}

impl ScoreError {
    pub(crate) fn new(code: ScoreErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(ScoreErrorCode::InvalidRecord, message)
    }

    pub(crate) fn catalog(message: impl Into<String>) -> Self {
        Self::new(ScoreErrorCode::Catalog, message)
    }

    pub fn code(&self) -> ScoreErrorCode {
        self.code
    }
}

impl fmt::Display for ScoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ScoreError {}

impl From<RatingError> for ScoreError {
    fn from(error: RatingError) -> Self {
        Self::new(ScoreErrorCode::Rating, error.to_string())
    }
}
