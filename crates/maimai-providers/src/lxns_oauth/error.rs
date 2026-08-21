use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthErrorCode {
    InvalidConfiguration,
    InvalidRequest,
    EntropyUnavailable,
    Timeout,
    Network,
    Unauthorized,
    InvalidGrant,
    Http,
    InvalidTokenResponse,
    RefreshTokenNotRotated,
}

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct OAuthError {
    code: OAuthErrorCode,
    message: String,
    status: Option<u16>,
    body: Option<String>,
}

impl OAuthError {
    pub(super) fn new(code: OAuthErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            body: None,
        }
    }

    pub(super) fn response(
        code: OAuthErrorCode,
        message: impl Into<String>,
        status: u16,
        body: Option<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            status: Some(status),
            body,
        }
    }

    pub fn code(&self) -> OAuthErrorCode {
        self.code
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}
