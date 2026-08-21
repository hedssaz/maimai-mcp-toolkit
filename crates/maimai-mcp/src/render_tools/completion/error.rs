use maimai_app::completion::CompletionError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompletionToolError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Completion(#[from] CompletionError),
    #[error("完成表结果序列化失败")]
    Serialization,
}

impl CompletionToolError {
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
