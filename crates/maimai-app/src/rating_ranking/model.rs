use std::path::PathBuf;

use maimai_core::QqId;
use time::OffsetDateTime;

use super::RatingRankingError;

pub const MAX_RANKING_USERNAME_CHARS: usize = 128;
pub const MAX_RANKING_RANGE: usize = 30;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingRankingUsername(String);

impl RatingRankingUsername {
    pub fn new(value: impl Into<String>) -> Result<Self, RatingRankingError> {
        let value = value.into();
        if value.chars().any(char::is_control) {
            return Err(RatingRankingError::invalid(
                "name/username 不能包含控制字符",
            ));
        }
        let value = value.trim().to_owned();
        if value.is_empty() {
            return Err(RatingRankingError::invalid("name/username 不能为空"));
        }
        if value.chars().count() > MAX_RANKING_USERNAME_CHARS {
            return Err(RatingRankingError::invalid(format!(
                "name/username 最多 {MAX_RANKING_USERNAME_CHARS} 个字符"
            )));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingRankingTarget(TargetKind);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TargetKind {
    Username(RatingRankingUsername),
    Qq(QqId),
    Range { start: usize, end: usize },
    Page(usize),
}

impl RatingRankingTarget {
    pub fn username(username: RatingRankingUsername) -> Self {
        Self(TargetKind::Username(username))
    }

    pub fn qq(qq: QqId) -> Self {
        Self(TargetKind::Qq(qq))
    }

    pub fn page(page: usize) -> Result<Self, RatingRankingError> {
        if page == 0 {
            return Err(RatingRankingError::invalid("page 必须大于 0"));
        }
        Ok(Self(TargetKind::Page(page)))
    }

    pub fn range(start: usize, end: usize) -> Result<Self, RatingRankingError> {
        if start == 0 || end < start {
            return Err(RatingRankingError::invalid(
                "startRank/endRank 参数不合法。",
            ));
        }
        let count = end
            .checked_sub(start)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| RatingRankingError::invalid("startRank/endRank 参数不合法。"))?;
        if count > MAX_RANKING_RANGE {
            return Err(RatingRankingError::invalid(
                "Diving-Fish 公开排名一次最多输出 30 人。",
            ));
        }
        Ok(Self(TargetKind::Range { start, end }))
    }

    pub(super) fn qq_value(&self) -> Option<&QqId> {
        match &self.0 {
            TargetKind::Qq(value) => Some(value),
            _ => None,
        }
    }

    pub(super) fn kind(&self) -> &TargetKind {
        &self.0
    }
}

pub struct RatingRankingRequest {
    pub target: RatingRankingTarget,
    pub now: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingRankingImage {
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
}
