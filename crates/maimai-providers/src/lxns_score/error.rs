use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LxnsScoreErrorCode {
    InvalidConfiguration,
    InvalidRequest,
    Unauthorized,
    Timeout,
    Network,
    Redirect,
    Http,
    Api,
    InvalidJson,
    InvalidShape,
    ResponseTooLarge,
}

#[derive(Clone, Debug, Error)]
#[error("{message}")]
pub struct LxnsScoreError {
    code: LxnsScoreErrorCode,
    message: String,
    status: Option<u16>,
    body: Option<String>,
}

impl LxnsScoreError {
    pub(super) fn new(code: LxnsScoreErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            body: None,
        }
    }

    pub(super) fn response(
        code: LxnsScoreErrorCode,
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

    pub fn code(&self) -> LxnsScoreErrorCode {
        self.code
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    pub fn is_player_not_found(&self) -> bool {
        self.status == Some(404)
            && self
                .body
                .as_deref()
                .is_some_and(|body| body.to_ascii_lowercase().contains("player not found"))
    }
}
