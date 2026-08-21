use thiserror::Error;

use crate::{image_output::ImageOutputError, score_service::PlayerScoreServiceError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreListErrorCode {
    InvalidInput,
    Provider,
    Storage,
    Catalog,
    AssetsRequired,
    Render,
    Output,
    TaskJoin,
}

#[derive(Debug, Error)]
pub enum ScoreListError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("谱面缺少 Diving-Fish 曲库投影: {0:?}")]
    MissingDivingFishChart(maimai_core::ChartKey),
    #[error(transparent)]
    CatalogLookup(#[from] maimai_catalog::CatalogLookupError),
    #[error(transparent)]
    Score(#[from] PlayerScoreServiceError),
    #[error("成绩列表渲染任务异常终止")]
    TaskJoin,
    #[error(transparent)]
    Render(#[from] maimai_render::ScoreListRenderError),
    #[error(transparent)]
    Output(#[from] ImageOutputError),
}

impl ScoreListError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn code(&self) -> ScoreListErrorCode {
        match self {
            Self::InvalidInput(_) => ScoreListErrorCode::InvalidInput,
            Self::MissingDivingFishChart(_) | Self::CatalogLookup(_) => ScoreListErrorCode::Catalog,
            Self::Score(error) => match error.code() {
                crate::score_service::PlayerScoreServiceErrorCode::InvalidLookup
                | crate::score_service::PlayerScoreServiceErrorCode::UnsupportedSource => {
                    ScoreListErrorCode::InvalidInput
                }
                crate::score_service::PlayerScoreServiceErrorCode::Storage => {
                    ScoreListErrorCode::Storage
                }
                crate::score_service::PlayerScoreServiceErrorCode::Catalog => {
                    ScoreListErrorCode::Catalog
                }
                crate::score_service::PlayerScoreServiceErrorCode::SourceUnavailable
                | crate::score_service::PlayerScoreServiceErrorCode::AuthRequired
                | crate::score_service::PlayerScoreServiceErrorCode::Provider => {
                    ScoreListErrorCode::Provider
                }
            },
            Self::TaskJoin => ScoreListErrorCode::TaskJoin,
            Self::Render(error) if error.assets_required() => ScoreListErrorCode::AssetsRequired,
            Self::Render(_) => ScoreListErrorCode::Render,
            Self::Output(_) => ScoreListErrorCode::Output,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Score(error) => error.status(),
            _ => None,
        }
    }
}
