use maimai_app::rise_score::{RiseScoreError, RiseScoreErrorCode};
use thiserror::Error;

#[derive(Debug, Error)]
pub(super) enum RiseScoreToolError {
    #[error("INVALID_INPUT: {0}")]
    InvalidInput(String),
    #[error("{code}: {source}")]
    Application {
        code: &'static str,
        #[source]
        source: RiseScoreError,
    },
    #[error("INTERNAL_ERROR: 渲染结果序列化失败")]
    Serialization,
}

impl RiseScoreToolError {
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}

impl From<RiseScoreError> for RiseScoreToolError {
    fn from(source: RiseScoreError) -> Self {
        let code = match source.code() {
            RiseScoreErrorCode::InvalidInput => "INVALID_INPUT",
            RiseScoreErrorCode::Provider => "PROVIDER_ERROR",
            RiseScoreErrorCode::Storage => "STORAGE_ERROR",
            RiseScoreErrorCode::Catalog => "CATALOG_ERROR",
            RiseScoreErrorCode::AssetsRequired => "RISE_SCORE_ASSETS_REQUIRED",
            RiseScoreErrorCode::Render => "RENDER_ERROR",
            RiseScoreErrorCode::Output => "OUTPUT_STORE_ERROR",
            RiseScoreErrorCode::TaskJoin => "RENDER_TASK_ERROR",
        };
        Self::Application { code, source }
    }
}
