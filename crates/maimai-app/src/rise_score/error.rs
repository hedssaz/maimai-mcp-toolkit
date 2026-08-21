use thiserror::Error;

use crate::{
    image_output::ImageOutputError,
    score_service::{PlayerScoreServiceError, PlayerScoreServiceErrorCode},
    scores::ScoreError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiseScoreErrorCode {
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
pub enum RiseScoreError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("没有推荐的谱面")]
    NoRecommendations,
    #[error(transparent)]
    ScoreService(#[from] PlayerScoreServiceError),
    #[error(transparent)]
    Score(#[from] ScoreError),
    #[error("曲库查询失败")]
    CatalogQuery(#[source] maimai_catalog::QueryError),
    #[error("rating 计算失败")]
    Rating(#[from] maimai_core::RatingError),
    #[error("上分推荐渲染任务异常终止")]
    TaskJoin,
    #[error(transparent)]
    Render(#[from] maimai_render::RiseScoreRenderError),
    #[error(transparent)]
    Output(#[from] ImageOutputError),
}

impl RiseScoreError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn code(&self) -> RiseScoreErrorCode {
        match self {
            Self::InvalidInput(_) | Self::NoRecommendations => RiseScoreErrorCode::InvalidInput,
            Self::ScoreService(error) => score_service_code(error.code()),
            Self::Score(_) | Self::CatalogQuery(_) | Self::Rating(_) => RiseScoreErrorCode::Catalog,
            Self::TaskJoin => RiseScoreErrorCode::TaskJoin,
            Self::Render(error) if error.assets_required() => RiseScoreErrorCode::AssetsRequired,
            Self::Render(_) => RiseScoreErrorCode::Render,
            Self::Output(_) => RiseScoreErrorCode::Output,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::ScoreService(error) => error.status(),
            _ => None,
        }
    }
}

fn score_service_code(code: PlayerScoreServiceErrorCode) -> RiseScoreErrorCode {
    match code {
        PlayerScoreServiceErrorCode::InvalidLookup
        | PlayerScoreServiceErrorCode::UnsupportedSource => RiseScoreErrorCode::InvalidInput,
        PlayerScoreServiceErrorCode::Storage => RiseScoreErrorCode::Storage,
        PlayerScoreServiceErrorCode::Catalog => RiseScoreErrorCode::Catalog,
        PlayerScoreServiceErrorCode::SourceUnavailable
        | PlayerScoreServiceErrorCode::AuthRequired
        | PlayerScoreServiceErrorCode::Provider => RiseScoreErrorCode::Provider,
    }
}
