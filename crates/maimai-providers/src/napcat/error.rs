use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NapCatErrorCode {
    InvalidConfiguration,
    InvalidRequest,
    Timeout,
    Network,
    Http,
    OneBot,
    InvalidResponse,
}

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct NapCatError {
    code: NapCatErrorCode,
    message: String,
    http_status: Option<u16>,
    onebot_status: Option<String>,
    retcode: Option<i64>,
    provider_message: Option<String>,
    body: Option<String>,
}

impl NapCatError {
    pub(super) fn new(code: NapCatErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            http_status: None,
            onebot_status: None,
            retcode: None,
            provider_message: None,
            body: None,
        }
    }

    pub(super) fn http(status: u16, body: Option<String>) -> Self {
        Self {
            code: NapCatErrorCode::Http,
            message: format!("NapCat 请求失败：HTTP {status}"),
            http_status: Some(status),
            onebot_status: None,
            retcode: None,
            provider_message: None,
            body,
        }
    }

    pub(super) fn onebot(
        status: Option<String>,
        retcode: Option<i64>,
        provider_message: Option<String>,
        body: Option<String>,
    ) -> Self {
        Self {
            code: NapCatErrorCode::OneBot,
            message: "NapCat OneBot 请求被拒绝".to_owned(),
            http_status: None,
            onebot_status: status,
            retcode,
            provider_message,
            body,
        }
    }

    pub(super) fn invalid_response(body: Option<String>) -> Self {
        Self {
            code: NapCatErrorCode::InvalidResponse,
            message: "NapCat 响应结构不正确".to_owned(),
            http_status: None,
            onebot_status: None,
            retcode: None,
            provider_message: None,
            body,
        }
    }

    pub(super) fn response_too_large() -> Self {
        Self::new(NapCatErrorCode::InvalidResponse, "NapCat 响应超过大小限制")
    }

    pub fn code(&self) -> NapCatErrorCode {
        self.code
    }

    pub fn http_status(&self) -> Option<u16> {
        self.http_status
    }

    pub fn onebot_status(&self) -> Option<&str> {
        self.onebot_status.as_deref()
    }

    pub fn retcode(&self) -> Option<i64> {
        self.retcode
    }

    pub fn provider_message(&self) -> Option<&str> {
        self.provider_message.as_deref()
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}
