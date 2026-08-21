use maimai_app::rankings::{RankingError, RankingErrorCode};
use serde_json::json;

use crate::{DispatchError, ToolFailure};

#[derive(Debug)]
pub struct RankingToolError {
    code: &'static str,
    message: String,
    status: Option<u16>,
    body: Option<String>,
}

impl RankingToolError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_INPUT",
            message: message.into(),
            status: None,
            body: None,
        }
    }

    pub fn internal() -> Self {
        Self {
            code: "INTERNAL_ERROR",
            message: "群榜结果序列化失败。".to_owned(),
            status: None,
            body: None,
        }
    }
}

impl From<RankingError> for RankingToolError {
    fn from(error: RankingError) -> Self {
        let code = match error.code() {
            RankingErrorCode::InvalidInput => "INVALID_INPUT",
            RankingErrorCode::NotFound => "NOT_FOUND",
            RankingErrorCode::Ambiguous => "AMBIGUOUS_IDENTITY",
            RankingErrorCode::Timeout => "TIMEOUT",
            RankingErrorCode::Network => "NETWORK_ERROR",
            RankingErrorCode::Http => "HTTP_ERROR",
            RankingErrorCode::Provider => "PROVIDER_ERROR",
            RankingErrorCode::Storage => "STORAGE_ERROR",
            RankingErrorCode::Catalog => "SONG_NOT_FOUND",
            RankingErrorCode::Task => "UNKNOWN_ERROR",
        };
        Self {
            code,
            message: error.to_string(),
            status: error.status(),
            body: error.body().map(str::to_owned),
        }
    }
}

impl From<RankingToolError> for DispatchError {
    fn from(error: RankingToolError) -> Self {
        let text = error.message.clone();
        let structured = json!({
            "error": {
                "code": error.code,
                "message": error.message,
                "status": error.status,
                "body": error.body,
            }
        });
        ToolFailure::text(text)
            .with_structured_content(structured)
            .into()
    }
}
