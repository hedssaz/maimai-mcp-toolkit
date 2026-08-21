use maimai_app::music_info::MusicInfoError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderToolError {
    #[error("{0}")]
    Invalid(String),

    #[error(transparent)]
    MusicInfo(#[from] MusicInfoError),

    #[error("渲染结果序列化失败")]
    Serialization,
}

impl RenderToolError {
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
