use crate::{identity::IdentityError, score_service::PlayerScoreServiceError};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreBySongErrorCode {
    InvalidInput,
    AmbiguousIdentity,
    SongNotFound,
    MusicIdNotFound,
    Identity,
    Catalog,
    ScoreQueryFailed,
}

#[derive(Debug, Error)]
pub enum ScoreBySongError {
    #[error("单曲成绩查询参数不正确：{field}")]
    InvalidInput { field: &'static str },
    #[error("target 匹配多个 QQ，请提供明确 QQ。")]
    AmbiguousIdentity,
    #[error("没有找到匹配曲目。")]
    SongNotFound,
    #[error("匹配曲目没有可用于水鱼查询的 music_id。")]
    MusicIdNotFound,
    #[error("QQ 身份缓存查询失败。")]
    Identity(#[source] IdentityError),
    #[error("本地曲库查询失败。")]
    Catalog,
    #[error("水鱼 MCP 未返回任何成功的单曲成绩。")]
    ScoreQueryFailed(#[source] PlayerScoreServiceError),
}

impl ScoreBySongError {
    pub fn code(&self) -> ScoreBySongErrorCode {
        match self {
            Self::InvalidInput { .. } => ScoreBySongErrorCode::InvalidInput,
            Self::AmbiguousIdentity => ScoreBySongErrorCode::AmbiguousIdentity,
            Self::SongNotFound => ScoreBySongErrorCode::SongNotFound,
            Self::MusicIdNotFound => ScoreBySongErrorCode::MusicIdNotFound,
            Self::Identity(_) => ScoreBySongErrorCode::Identity,
            Self::Catalog => ScoreBySongErrorCode::Catalog,
            Self::ScoreQueryFailed(_) => ScoreBySongErrorCode::ScoreQueryFailed,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::ScoreQueryFailed(error) => error.status(),
            _ => None,
        }
    }

    pub fn body(&self) -> Option<&str> {
        match self {
            Self::ScoreQueryFailed(error) => error.body(),
            _ => None,
        }
    }
}

impl From<IdentityError> for ScoreBySongError {
    fn from(value: IdentityError) -> Self {
        Self::Identity(value)
    }
}
