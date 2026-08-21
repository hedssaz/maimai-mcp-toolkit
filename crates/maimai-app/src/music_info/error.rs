use thiserror::Error;

#[derive(Debug, Error)]
pub enum MusicInfoError {
    #[error("{0}")]
    InvalidInput(String),

    #[error("未找到曲目: {0}")]
    NotFound(String),

    #[error("匹配到多个曲目，请指定更精确的曲名或 ID:\n{0}")]
    Ambiguous(String),

    #[error("批量曲目信息最多支持 50 项")]
    BatchTooLarge,

    #[error("曲目信息渲染任务异常终止")]
    TaskJoin,

    #[error(transparent)]
    Render(#[from] maimai_render::RenderError),

    #[error(transparent)]
    Output(#[from] crate::image_output::ImageOutputError),
}

impl MusicInfoError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}
