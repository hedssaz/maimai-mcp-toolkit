use thiserror::Error;

#[derive(Debug, Error)]
pub enum MusicGlobalStatsError {
    #[error("{0}")]
    InvalidInput(String),

    #[error(transparent)]
    MusicResolution(#[from] crate::music_info::MusicInfoError),

    #[error("全服统计渲染任务异常终止")]
    TaskJoin,

    #[error(transparent)]
    Render(#[from] maimai_render::RenderError),

    #[error(transparent)]
    Output(#[from] crate::image_output::ImageOutputError),
}

impl MusicGlobalStatsError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}
