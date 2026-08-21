use std::{error::Error, fmt};

use crate::{
    b50_image::B50ImageError,
    score_service::{PlayerScoreServiceError, PlayerScoreServiceErrorCode},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B50RenderErrorCode {
    InvalidInput,
    UnsafePath,
    StaticOverrideUnsupported,
    PlayerNotFound,
    SourceUnavailable,
    AuthRequired,
    Provider,
    Storage,
    Catalog,
    AssetsRequired,
    Render,
    Output,
    TaskJoin,
}

pub enum B50RenderError {
    InvalidInput(String),
    UnsafePath { field: &'static str },
    LegacyStaticOverrideUnsupported,
    PlayerNotFound,
    Score(PlayerScoreServiceError),
    Image(B50ImageError),
    TaskJoin,
}

impl B50RenderError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn code(&self) -> B50RenderErrorCode {
        match self {
            Self::InvalidInput(_) => B50RenderErrorCode::InvalidInput,
            Self::UnsafePath { .. } => B50RenderErrorCode::UnsafePath,
            Self::LegacyStaticOverrideUnsupported => B50RenderErrorCode::StaticOverrideUnsupported,
            Self::PlayerNotFound => B50RenderErrorCode::PlayerNotFound,
            Self::Score(error) => match error.code() {
                PlayerScoreServiceErrorCode::InvalidLookup
                | PlayerScoreServiceErrorCode::UnsupportedSource => {
                    B50RenderErrorCode::InvalidInput
                }
                PlayerScoreServiceErrorCode::SourceUnavailable => {
                    B50RenderErrorCode::SourceUnavailable
                }
                PlayerScoreServiceErrorCode::AuthRequired => B50RenderErrorCode::AuthRequired,
                PlayerScoreServiceErrorCode::Provider => B50RenderErrorCode::Provider,
                PlayerScoreServiceErrorCode::Storage => B50RenderErrorCode::Storage,
                PlayerScoreServiceErrorCode::Catalog => B50RenderErrorCode::Catalog,
            },
            Self::Image(error) if error.missing_assets().is_some() => {
                B50RenderErrorCode::AssetsRequired
            }
            Self::Image(error) => match error.code() {
                "OUTPUT_STORE_ERROR" => B50RenderErrorCode::Output,
                _ => B50RenderErrorCode::Render,
            },
            Self::TaskJoin => B50RenderErrorCode::TaskJoin,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Score(error) => error.status(),
            _ => None,
        }
    }

    pub fn body(&self) -> Option<&str> {
        match self {
            Self::Score(error) => error.body(),
            _ => None,
        }
    }
}

impl fmt::Debug for B50RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("B50RenderError")
            .field("code", &self.code())
            .field("status", &self.status())
            .field("has_body", &self.body().is_some())
            .finish()
    }
}

impl fmt::Display for B50RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => formatter.write_str(message),
            Self::UnsafePath { field } => {
                write!(formatter, "{field} 不在配置允许的资源根目录内。")
            }
            Self::LegacyStaticOverrideUnsupported => formatter
                .write_str("legacy 风格仅支持配置时加载的 staticDir；运行时不能切换资源根。"),
            Self::PlayerNotFound => formatter.write_str("未找到玩家数据"),
            Self::Score(error) => fmt::Display::fmt(error, formatter),
            Self::Image(error) => fmt::Display::fmt(error, formatter),
            Self::TaskJoin => formatter.write_str("B50 绘图任务异常终止"),
        }
    }
}

impl Error for B50RenderError {}

impl From<PlayerScoreServiceError> for B50RenderError {
    fn from(value: PlayerScoreServiceError) -> Self {
        Self::Score(value)
    }
}

impl From<B50ImageError> for B50RenderError {
    fn from(value: B50ImageError) -> Self {
        Self::Image(value)
    }
}
