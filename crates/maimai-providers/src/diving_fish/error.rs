use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorCode {
    InvalidConfiguration,
    InvalidRequest,
    AuthRequired,
    ConfirmationRequired,
    Timeout,
    Network,
    Redirect,
    BodyTooLarge,
    Http,
}

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct ProviderError {
    code: ProviderErrorCode,
    message: String,
    status: Option<u16>,
    body: Option<String>,
}

impl ProviderError {
    pub(super) fn new(code: ProviderErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            body: None,
        }
    }

    pub(super) fn http(status: u16, body: Option<String>) -> Self {
        Self {
            code: ProviderErrorCode::Http,
            message: format!("Diving-Fish 请求失败：HTTP {status}"),
            status: Some(status),
            body,
        }
    }

    pub(super) fn redirect(status: u16) -> Self {
        Self {
            code: ProviderErrorCode::Redirect,
            message: "Diving-Fish 请求返回重定向".to_owned(),
            status: Some(status),
            body: None,
        }
    }

    pub(super) fn body_too_large(status: u16) -> Self {
        Self {
            code: ProviderErrorCode::BodyTooLarge,
            message: "Diving-Fish 响应体超过安全上限".to_owned(),
            status: Some(status),
            body: None,
        }
    }

    pub fn code(&self) -> ProviderErrorCode {
        self.code
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}
