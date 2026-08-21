use maimai_app::score_list::{ScoreListError, ScoreListErrorCode};
use thiserror::Error;

#[derive(Debug, Error)]
pub(super) enum ScoreListToolError {
    #[error("INVALID_INPUT: {0}")]
    InvalidInput(String),
    #[error("{code}: {source}")]
    Application {
        code: &'static str,
        #[source]
        source: ScoreListError,
    },
    #[error("INTERNAL_ERROR: 渲染结果序列化失败")]
    Serialization,
}

impl ScoreListToolError {
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}

impl From<ScoreListError> for ScoreListToolError {
    fn from(source: ScoreListError) -> Self {
        let code = match source.code() {
            ScoreListErrorCode::InvalidInput => "INVALID_INPUT",
            ScoreListErrorCode::Provider => "PROVIDER_ERROR",
            ScoreListErrorCode::Storage => "STORAGE_ERROR",
            ScoreListErrorCode::Catalog => "CATALOG_ERROR",
            ScoreListErrorCode::AssetsRequired => "SCORE_LIST_ASSETS_REQUIRED",
            ScoreListErrorCode::Render => "RENDER_ERROR",
            ScoreListErrorCode::Output => "OUTPUT_STORE_ERROR",
            ScoreListErrorCode::TaskJoin => "RENDER_TASK_ERROR",
        };
        Self::Application { code, source }
    }
}
