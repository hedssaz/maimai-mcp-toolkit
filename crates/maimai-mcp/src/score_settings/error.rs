use maimai_app::score_settings::{ScoreSettingsError, ScoreSettingsErrorCode};
use serde_json::json;

use crate::{DispatchError, ToolFailure};

#[derive(Debug)]
pub struct ScoreSettingsToolError {
    code: &'static str,
    message: String,
}

impl ScoreSettingsToolError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_INPUT",
            message: message.into(),
        }
    }

    pub fn internal() -> Self {
        Self {
            code: "INTERNAL_ERROR",
            message: "成绩设置工具内部错误。".to_owned(),
        }
    }
}

impl From<ScoreSettingsError> for ScoreSettingsToolError {
    fn from(error: ScoreSettingsError) -> Self {
        match error {
            ScoreSettingsError::Public { code, message } => Self {
                code: match code {
                    ScoreSettingsErrorCode::InvalidInput => "INVALID_INPUT",
                    ScoreSettingsErrorCode::SourceNotAllowed => "SOURCE_NOT_ALLOWED",
                    ScoreSettingsErrorCode::TokenStorage => "TOKEN_STORE_ERROR",
                    ScoreSettingsErrorCode::SourcePreferenceStorage => "SOURCE_PREFERENCE_ERROR",
                },
                message: message.to_owned(),
            },
            ScoreSettingsError::TokenStorage(_) => Self {
                code: "TOKEN_STORE_ERROR",
                message: "Developer-Token 存储失败。".to_owned(),
            },
            ScoreSettingsError::SourcePreferenceStorage(_) => Self {
                code: "SOURCE_PREFERENCE_ERROR",
                message: "成绩数据源偏好存储失败。".to_owned(),
            },
        }
    }
}

impl From<ScoreSettingsError> for DispatchError {
    fn from(value: ScoreSettingsError) -> Self {
        ScoreSettingsToolError::from(value).into()
    }
}

impl From<ScoreSettingsToolError> for DispatchError {
    fn from(value: ScoreSettingsToolError) -> Self {
        let message = value.message.clone();
        ToolFailure::text(message)
            .with_structured_content(json!({
                "error": {
                    "code": value.code,
                    "message": value.message,
                    "status": null,
                    "body": null,
                }
            }))
            .into()
    }
}
