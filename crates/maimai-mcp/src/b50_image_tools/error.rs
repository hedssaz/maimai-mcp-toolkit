use maimai_app::b50_image::{B50ImageDataError, B50ImageError};
use serde_json::json;
use thiserror::Error;

use crate::{DispatchError, ToolFailure};

#[derive(Debug, Error)]
pub(super) enum B50ToolError {
    #[error("{0}")]
    InvalidInput(String),

    #[error(transparent)]
    Application(#[from] B50ImageError),

    #[error(transparent)]
    Data(#[from] B50ImageDataError),

    #[error("B50 请求超时。")]
    Timeout,

    #[error("B50 图片工具内部错误。")]
    Internal,
}

impl B50ToolError {
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::Application(error) => error.code(),
            Self::Data(error) => error.code(),
            Self::Timeout => "TIMEOUT",
            Self::Internal => "B50_IMAGE_ERROR",
        }
    }

    fn status(&self) -> Option<u16> {
        match self {
            Self::Data(error) => error.status(),
            _ => None,
        }
    }
}

impl From<B50ToolError> for DispatchError {
    fn from(value: B50ToolError) -> Self {
        let body = match &value {
            B50ToolError::Application(error) => error
                .missing_assets()
                .and_then(|missing| serde_json::to_string(&json!({"missing": missing})).ok()),
            B50ToolError::Data(error) => error
                .ambiguous_qqs()
                .and_then(|qqs| serde_json::to_string(&json!({"qqs":qqs})).ok()),
            _ => None,
        };
        let status = value.status();
        let message = value.to_string();
        let structured = json!({
            "error": {
                "code": value.code(),
                "message": message,
                "status": status,
                "body": body,
            }
        });
        ToolFailure::text(message)
            .with_structured_content(structured)
            .into()
    }
}
