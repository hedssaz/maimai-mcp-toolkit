use maimai_providers::{OAuthError, OAuthErrorCode};
use maimai_storage::StorageError;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OAuthServiceErrorCode {
    ConfigMissing,
    InvalidInput,
    InvalidState,
    AuthRequired,
    AuthorizationNotFound,
    AuthorizationExpired,
    StateMismatch,
    ContextMismatch,
    OAuthRejected,
    ExchangeBusy,
    Timeout,
    AuthorizationLost,
    Provider,
    Storage,
}

#[derive(Debug, Error)]
pub enum OAuthServiceError {
    #[error("{message}")]
    Public {
        code: OAuthServiceErrorCode,
        message: &'static str,
    },

    #[error("LXNS OAuth provider request failed")]
    Provider(#[source] OAuthError),

    #[error("LXNS OAuth storage operation failed")]
    Storage(#[source] StorageError),
}

impl OAuthServiceError {
    pub fn code(&self) -> OAuthServiceErrorCode {
        match self {
            Self::Public { code, .. } => *code,
            Self::Provider(error) => match error.code() {
                OAuthErrorCode::InvalidRequest | OAuthErrorCode::InvalidConfiguration => {
                    OAuthServiceErrorCode::InvalidInput
                }
                OAuthErrorCode::Timeout => OAuthServiceErrorCode::Timeout,
                _ => OAuthServiceErrorCode::Provider,
            },
            Self::Storage(_) => OAuthServiceErrorCode::Storage,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Provider(error) => error.status(),
            _ => None,
        }
    }

    pub fn body(&self) -> Option<&str> {
        match self {
            Self::Provider(error) => error.body(),
            _ => None,
        }
    }

    pub(super) const fn public(code: OAuthServiceErrorCode, message: &'static str) -> Self {
        Self::Public { code, message }
    }
}

impl From<OAuthError> for OAuthServiceError {
    fn from(value: OAuthError) -> Self {
        Self::Provider(value)
    }
}

impl From<StorageError> for OAuthServiceError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}
