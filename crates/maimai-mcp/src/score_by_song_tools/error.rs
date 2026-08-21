use maimai_app::score_by_song::{ScoreBySongError, ScoreBySongErrorCode};
use serde_json::{Value, json};

use crate::{DispatchError, ToolFailure};

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ScoreBySongToolError {
    code: &'static str,
    message: String,
    data: Option<Value>,
}

impl ScoreBySongToolError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_INPUT", message)
    }

    pub fn timeout() -> Self {
        Self::new("TIMEOUT", "单曲成绩查询超时。")
    }

    pub fn internal() -> Self {
        Self::new("INTERNAL_ERROR", "单曲成绩查询工具内部错误。")
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

impl From<ScoreBySongError> for ScoreBySongToolError {
    fn from(error: ScoreBySongError) -> Self {
        let code = match error.code() {
            ScoreBySongErrorCode::InvalidInput | ScoreBySongErrorCode::AmbiguousIdentity => {
                "INVALID_INPUT"
            }
            ScoreBySongErrorCode::SongNotFound => "SONG_NOT_FOUND",
            ScoreBySongErrorCode::MusicIdNotFound => "MUSIC_ID_NOT_FOUND",
            ScoreBySongErrorCode::Identity => "IDENTITY_ERROR",
            ScoreBySongErrorCode::Catalog => "SONG_SEARCH_FAILED",
            ScoreBySongErrorCode::ScoreQueryFailed => "SCORE_QUERY_FAILED",
        };
        let data = (error.status().is_some() || error.body().is_some()).then(|| {
            json!({
                "status": error.status(),
                "body": error.body(),
            })
        });
        Self {
            code,
            message: error.to_string(),
            data,
        }
    }
}

impl From<ScoreBySongToolError> for DispatchError {
    fn from(value: ScoreBySongToolError) -> Self {
        let text = value.message.clone();
        ToolFailure::text(text)
            .with_structured_content(json!({
                "error": {
                    "code": value.code,
                    "message": value.message,
                    "data": value.data,
                }
            }))
            .into()
    }
}
