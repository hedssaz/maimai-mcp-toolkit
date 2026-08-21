use maimai_app::{
    identity::IdentityError,
    score_service::{PlayerScoreServiceError, PlayerScoreServiceErrorCode},
    score_settings::ScoreSettingsError,
};
use maimai_providers::ProviderError;
use serde_json::json;

use crate::{DispatchError, ToolFailure};

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ScoreQueryToolError {
    code: &'static str,
    message: String,
    status: Option<u16>,
    body: Option<String>,
}

impl ScoreQueryToolError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_INPUT", message)
    }

    pub fn timeout() -> Self {
        Self::new("TIMEOUT", "成绩查询超时。")
    }

    pub fn internal() -> Self {
        Self::new("INTERNAL_ERROR", "成绩查询工具内部错误。")
    }

    pub fn provider(error: ProviderError) -> Self {
        Self {
            code: "PROVIDER_ERROR",
            message: "Diving-Fish API 请求失败。".to_owned(),
            status: error.status(),
            body: error.body().map(str::to_owned),
        }
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            body: None,
        }
    }

    pub(crate) fn structured(&self) -> serde_json::Value {
        json!({
            "code": self.code,
            "message": self.message,
            "status": self.status,
            "body": self.body,
        })
    }
}

impl From<PlayerScoreServiceError> for ScoreQueryToolError {
    fn from(error: PlayerScoreServiceError) -> Self {
        let code = match error.code() {
            PlayerScoreServiceErrorCode::InvalidLookup => "INVALID_LOOKUP",
            PlayerScoreServiceErrorCode::UnsupportedSource => "SOURCE_NOT_ALLOWED",
            PlayerScoreServiceErrorCode::SourceUnavailable => "SOURCE_EMPTY",
            PlayerScoreServiceErrorCode::AuthRequired => "AUTH_REQUIRED",
            PlayerScoreServiceErrorCode::Provider => "PROVIDER_ERROR",
            PlayerScoreServiceErrorCode::Storage => "STORAGE_ERROR",
            PlayerScoreServiceErrorCode::Catalog => "CATALOG_ERROR",
        };
        Self {
            code,
            message: error.to_string(),
            status: error.status(),
            body: error.body().map(str::to_owned),
        }
    }
}

impl From<IdentityError> for ScoreQueryToolError {
    fn from(_error: IdentityError) -> Self {
        Self::new("IDENTITY_ERROR", "QQ 身份缓存查询失败。")
    }
}

impl From<ScoreSettingsError> for ScoreQueryToolError {
    fn from(_error: ScoreSettingsError) -> Self {
        Self::new("TOKEN_STORE_ERROR", "Developer-Token 读取失败。")
    }
}

impl From<ScoreQueryToolError> for DispatchError {
    fn from(value: ScoreQueryToolError) -> Self {
        let text = value.message.clone();
        let structured = value.structured();
        ToolFailure::text(text)
            .with_structured_content(json!({"error": structured}))
            .into()
    }
}
