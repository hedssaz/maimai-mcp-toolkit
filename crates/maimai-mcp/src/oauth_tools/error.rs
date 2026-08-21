use maimai_app::oauth::{OAuthServiceError, OAuthServiceErrorCode};
use maimai_providers::OAuthErrorCode;
use serde_json::json;

use crate::{DispatchError, ToolFailure};

#[derive(Debug)]
pub struct OAuthToolError {
    code: &'static str,
    message: String,
    status: Option<u16>,
}

impl OAuthToolError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_INPUT",
            message: message.into(),
            status: None,
        }
    }

    pub fn internal() -> Self {
        Self {
            code: "INTERNAL_ERROR",
            message: "落雪 OAuth 内部错误。".to_owned(),
            status: None,
        }
    }
}

impl From<OAuthServiceError> for OAuthToolError {
    fn from(error: OAuthServiceError) -> Self {
        match error {
            OAuthServiceError::Public { code, message } => Self {
                code: service_code(code),
                message: message.to_owned(),
                status: None,
            },
            OAuthServiceError::Provider(error) => Self {
                code: provider_code(error.code()),
                message: error.to_string(),
                status: error.status(),
            },
            OAuthServiceError::Storage(_) => Self {
                code: "STORE_ERROR",
                message: "OAuth 状态存储失败。".to_owned(),
                status: None,
            },
        }
    }
}

impl From<OAuthServiceError> for DispatchError {
    fn from(value: OAuthServiceError) -> Self {
        OAuthToolError::from(value).into()
    }
}

impl From<OAuthToolError> for DispatchError {
    fn from(value: OAuthToolError) -> Self {
        let message = value.message.clone();
        let structured = match value.status {
            Some(status) => json!({
                "code": value.code,
                "message": value.message,
                "status": status,
            }),
            None => json!({"code": value.code, "message": value.message}),
        };
        ToolFailure::text(message)
            .with_structured_content(json!({"error": structured}))
            .into()
    }
}

const fn service_code(code: OAuthServiceErrorCode) -> &'static str {
    match code {
        OAuthServiceErrorCode::ConfigMissing => "CONFIG_MISSING",
        OAuthServiceErrorCode::InvalidInput => "INVALID_INPUT",
        OAuthServiceErrorCode::InvalidState | OAuthServiceErrorCode::StateMismatch => {
            "INVALID_STATE"
        }
        OAuthServiceErrorCode::AuthRequired => "AUTH_REQUIRED",
        OAuthServiceErrorCode::AuthorizationNotFound | OAuthServiceErrorCode::AuthorizationLost => {
            "INVALID_STATE"
        }
        OAuthServiceErrorCode::AuthorizationExpired => "STATE_EXPIRED",
        OAuthServiceErrorCode::ContextMismatch => "CONTEXT_MISMATCH",
        OAuthServiceErrorCode::OAuthRejected => "OAUTH_REJECTED",
        OAuthServiceErrorCode::ExchangeBusy => "EXCHANGE_BUSY",
        OAuthServiceErrorCode::Timeout => "TIMEOUT",
        OAuthServiceErrorCode::Provider => "OAUTH_ERROR",
        OAuthServiceErrorCode::Storage => "STORE_ERROR",
    }
}

const fn provider_code(code: OAuthErrorCode) -> &'static str {
    match code {
        OAuthErrorCode::InvalidConfiguration => "CONFIG_MISSING",
        OAuthErrorCode::InvalidRequest => "INVALID_INPUT",
        OAuthErrorCode::EntropyUnavailable => "ENTROPY_UNAVAILABLE",
        OAuthErrorCode::Timeout => "TIMEOUT",
        OAuthErrorCode::Network => "NETWORK_ERROR",
        OAuthErrorCode::Unauthorized => "UNAUTHORIZED",
        OAuthErrorCode::InvalidGrant => "INVALID_GRANT",
        OAuthErrorCode::Http => "HTTP_ERROR",
        OAuthErrorCode::InvalidTokenResponse => "INVALID_TOKEN_RESPONSE",
        OAuthErrorCode::RefreshTokenNotRotated => "INVALID_TOKEN_RESPONSE",
    }
}
