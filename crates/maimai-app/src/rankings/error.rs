use maimai_providers::{NapCatError, NapCatErrorCode};
use maimai_storage::{RankingJobError, RankingJobErrorCode, StorageError};
use thiserror::Error;

use crate::{identity::IdentityError, score_service::PlayerScoreServiceError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RankingErrorCode {
    InvalidInput,
    NotFound,
    Ambiguous,
    Timeout,
    Network,
    Http,
    Provider,
    Storage,
    Catalog,
    Task,
}

#[derive(Debug, Error)]
pub enum RankingError {
    #[error("群榜参数不正确：{field}")]
    InvalidInput { field: &'static str },
    #[error("没有找到匹配的群榜对象")]
    NotFound,
    #[error("群榜对象存在多个匹配")]
    Ambiguous,
    #[error("NapCat 群成员请求失败")]
    NapCat(#[source] NapCatError),
    #[error("玩家成绩请求失败")]
    Scores(#[source] PlayerScoreServiceError),
    #[error("QQ 身份查询失败")]
    Identity(#[source] IdentityError),
    #[error("群榜存储失败")]
    Storage(#[source] StorageError),
    #[error("曲目目录查询失败")]
    Catalog,
    #[error("群榜后台任务异常结束")]
    Task,
}

impl RankingError {
    pub fn code(&self) -> RankingErrorCode {
        match self {
            Self::InvalidInput { .. } => RankingErrorCode::InvalidInput,
            Self::NotFound => RankingErrorCode::NotFound,
            Self::Ambiguous => RankingErrorCode::Ambiguous,
            Self::NapCat(error) => match error.code() {
                NapCatErrorCode::Timeout => RankingErrorCode::Timeout,
                NapCatErrorCode::Network => RankingErrorCode::Network,
                NapCatErrorCode::Http => RankingErrorCode::Http,
                _ => RankingErrorCode::Provider,
            },
            Self::Scores(error) => match error.code() {
                crate::score_service::PlayerScoreServiceErrorCode::Storage => {
                    RankingErrorCode::Storage
                }
                crate::score_service::PlayerScoreServiceErrorCode::Catalog => {
                    RankingErrorCode::Catalog
                }
                _ => RankingErrorCode::Provider,
            },
            Self::Identity(_) => RankingErrorCode::Storage,
            Self::Storage(_) => RankingErrorCode::Storage,
            Self::Catalog => RankingErrorCode::Catalog,
            Self::Task => RankingErrorCode::Task,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::NapCat(error) => error.http_status(),
            Self::Scores(error) => error.status(),
            _ => None,
        }
    }

    pub fn body(&self) -> Option<&str> {
        match self {
            Self::NapCat(error) => error.body(),
            Self::Scores(error) => error.body(),
            _ => None,
        }
    }

    pub(crate) fn safe_job_error(&self) -> RankingJobError {
        let code = match self.code() {
            RankingErrorCode::InvalidInput => RankingJobErrorCode::InvalidInput,
            RankingErrorCode::Timeout => RankingJobErrorCode::Timeout,
            RankingErrorCode::Network => RankingJobErrorCode::Network,
            RankingErrorCode::Http => RankingJobErrorCode::Http,
            RankingErrorCode::Catalog => RankingJobErrorCode::Catalog,
            RankingErrorCode::Storage => RankingJobErrorCode::Storage,
            RankingErrorCode::NotFound
            | RankingErrorCode::Ambiguous
            | RankingErrorCode::Provider => RankingJobErrorCode::Provider,
            RankingErrorCode::Task => RankingJobErrorCode::Unknown,
        };
        RankingJobError {
            code,
            message: self.to_string(),
            status: self.status(),
            body: self.body().map(str::to_owned),
        }
    }
}

impl From<StorageError> for RankingError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<IdentityError> for RankingError {
    fn from(value: IdentityError) -> Self {
        Self::Identity(value)
    }
}
