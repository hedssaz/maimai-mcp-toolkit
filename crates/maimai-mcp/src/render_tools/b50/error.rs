use maimai_app::b50_render::{B50RenderError, B50RenderErrorCode};
use serde_json::json;

use crate::{DispatchError, ToolFailure};

#[derive(Debug, thiserror::Error)]
pub(super) enum B50RenderToolError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("渲染 B50 超时。")]
    Timeout,
    #[error("{0}")]
    Application(B50RenderError),
    #[error("渲染结果序列化失败")]
    Serialization,
}

impl B50RenderToolError {
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::Timeout => "TIMEOUT",
            Self::Serialization => "INTERNAL_ERROR",
            Self::Application(error) => match error.code() {
                B50RenderErrorCode::InvalidInput => "INVALID_INPUT",
                B50RenderErrorCode::UnsafePath => "UNSAFE_PATH",
                B50RenderErrorCode::StaticOverrideUnsupported => "STATIC_DIR_OVERRIDE_UNSUPPORTED",
                B50RenderErrorCode::PlayerNotFound => "PLAYER_NOT_FOUND",
                B50RenderErrorCode::SourceUnavailable => "SOURCE_EMPTY",
                B50RenderErrorCode::AuthRequired => "AUTH_REQUIRED",
                B50RenderErrorCode::Provider => "PROVIDER_ERROR",
                B50RenderErrorCode::Storage => "STORAGE_ERROR",
                B50RenderErrorCode::Catalog => "CATALOG_ERROR",
                B50RenderErrorCode::AssetsRequired => "ASSETS_REQUIRED",
                B50RenderErrorCode::Render => "RENDER_ERROR",
                B50RenderErrorCode::Output => "OUTPUT_STORE_ERROR",
                B50RenderErrorCode::TaskJoin => "RENDER_TASK_ERROR",
            },
        }
    }

    fn status(&self) -> Option<u16> {
        match self {
            Self::Application(error) => error.status(),
            _ => None,
        }
    }

    fn body(&self) -> Option<&str> {
        match self {
            Self::Application(error) => error.body(),
            _ => None,
        }
    }
}

impl From<B50RenderError> for B50RenderToolError {
    fn from(value: B50RenderError) -> Self {
        Self::Application(value)
    }
}

impl From<B50RenderToolError> for DispatchError {
    fn from(value: B50RenderToolError) -> Self {
        let message = match &value {
            B50RenderToolError::Application(B50RenderError::PlayerNotFound) => value.to_string(),
            B50RenderToolError::Application(_) => format!("渲染 B50 失败: {value}"),
            _ => value.to_string(),
        };
        let structured = json!({
            "error": {
                "code": value.code(),
                "message": message,
                "status": value.status(),
                "body": value.body(),
            }
        });
        ToolFailure::text(message)
            .with_structured_content(structured)
            .into()
    }
}
