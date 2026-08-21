use maimai_storage::StorageError;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreSettingsErrorCode {
    InvalidInput,
    SourceNotAllowed,
    TokenStorage,
    SourcePreferenceStorage,
}

#[derive(Debug, Error)]
pub enum ScoreSettingsError {
    #[error("{message}")]
    Public {
        code: ScoreSettingsErrorCode,
        message: &'static str,
    },

    #[error("developer token storage operation failed")]
    TokenStorage(#[source] StorageError),

    #[error("score source preference storage operation failed")]
    SourcePreferenceStorage(#[source] StorageError),
}

impl ScoreSettingsError {
    pub const fn code(&self) -> ScoreSettingsErrorCode {
        match self {
            Self::Public { code, .. } => *code,
            Self::TokenStorage(_) => ScoreSettingsErrorCode::TokenStorage,
            Self::SourcePreferenceStorage(_) => ScoreSettingsErrorCode::SourcePreferenceStorage,
        }
    }

    pub(super) const fn public(code: ScoreSettingsErrorCode, message: &'static str) -> Self {
        Self::Public { code, message }
    }
}
