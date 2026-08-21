use thiserror::Error;

#[derive(Debug, Error)]
pub enum RatingRankingError {
    #[error("{0}")]
    InvalidInput(String),

    #[error("QQ {qq} 查询结果没有水鱼 username，无法定位 Diving-Fish 公开排名。")]
    MissingUsername { qq: String },

    #[error("Diving-Fish rating ranking 返回了无效用户名")]
    InvalidProviderUsername,

    #[error(transparent)]
    Provider(#[from] maimai_providers::DivingFishScoreError),

    #[error("rating 排行榜渲染任务异常终止")]
    TaskJoin,

    #[error(transparent)]
    Render(#[from] maimai_render::RenderError),

    #[error(transparent)]
    Output(#[from] crate::image_output::ImageOutputError),
}

impl RatingRankingError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Provider(error) => error.status(),
            _ => None,
        }
    }

    pub fn body(&self) -> Option<&str> {
        match self {
            Self::Provider(error) => error.body(),
            _ => None,
        }
    }
}
