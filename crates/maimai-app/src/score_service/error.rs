use std::{error::Error, fmt};

use maimai_providers::RawJsonError;
use maimai_providers::{DivingFishScoreError, LxnsScoreError, LxnsScoreErrorCode};

use crate::{
    oauth::{OAuthServiceError, OAuthServiceErrorCode},
    scores::{ScoreError, ScoreErrorCode},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerScoreServiceErrorCode {
    InvalidLookup,
    UnsupportedSource,
    SourceUnavailable,
    AuthRequired,
    Provider,
    Storage,
    Catalog,
}

pub struct PlayerScoreServiceError {
    code: PlayerScoreServiceErrorCode,
    message: &'static str,
    status: Option<u16>,
    body: Option<String>,
}

impl PlayerScoreServiceError {
    pub(crate) fn new(code: PlayerScoreServiceErrorCode, message: &'static str) -> Self {
        Self {
            code,
            message,
            status: None,
            body: None,
        }
    }

    pub(crate) fn source_unavailable(message: &'static str) -> Self {
        Self::new(PlayerScoreServiceErrorCode::SourceUnavailable, message)
    }

    pub(crate) fn storage() -> Self {
        Self::new(PlayerScoreServiceErrorCode::Storage, "读取本地成绩状态失败")
    }

    pub(crate) fn diving_fish(error: DivingFishScoreError) -> Self {
        Self {
            code: PlayerScoreServiceErrorCode::Provider,
            message: "Diving-Fish 成绩请求失败",
            status: error.status(),
            body: error.body().map(str::to_owned),
        }
    }

    pub(crate) fn lxns(error: LxnsScoreError) -> Self {
        let code = if error.code() == LxnsScoreErrorCode::Unauthorized {
            PlayerScoreServiceErrorCode::AuthRequired
        } else {
            PlayerScoreServiceErrorCode::Provider
        };
        Self {
            code,
            message: "LXNS 成绩请求失败",
            status: error.status(),
            body: error.body().map(str::to_owned),
        }
    }

    pub(crate) fn oauth(error: OAuthServiceError) -> Self {
        let code = match error.code() {
            OAuthServiceErrorCode::AuthRequired => PlayerScoreServiceErrorCode::AuthRequired,
            OAuthServiceErrorCode::InvalidInput | OAuthServiceErrorCode::InvalidState => {
                PlayerScoreServiceErrorCode::InvalidLookup
            }
            OAuthServiceErrorCode::Storage => PlayerScoreServiceErrorCode::Storage,
            _ => PlayerScoreServiceErrorCode::Provider,
        };
        Self {
            code,
            message: "LXNS OAuth 凭据获取失败",
            status: error.status(),
            body: error.body().map(str::to_owned),
        }
    }

    pub(crate) fn score(error: ScoreError) -> Self {
        let code = match error.code() {
            ScoreErrorCode::UnsupportedSource => PlayerScoreServiceErrorCode::UnsupportedSource,
            ScoreErrorCode::Catalog
            | ScoreErrorCode::ChartNotFound
            | ScoreErrorCode::AmbiguousChart => PlayerScoreServiceErrorCode::Catalog,
            ScoreErrorCode::InvalidRecord | ScoreErrorCode::Rating => {
                PlayerScoreServiceErrorCode::Provider
            }
        };
        Self::new(code, "成绩数据无法归一化")
    }

    pub(crate) fn raw_json(_error: RawJsonError) -> Self {
        Self::new(
            PlayerScoreServiceErrorCode::Provider,
            "成绩来源原始响应超过安全边界",
        )
    }

    pub fn code(&self) -> PlayerScoreServiceErrorCode {
        self.code
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}

impl fmt::Debug for PlayerScoreServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlayerScoreServiceError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("status", &self.status)
            .field("has_body", &self.body.is_some())
            .finish()
    }
}

impl fmt::Display for PlayerScoreServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for PlayerScoreServiceError {}
