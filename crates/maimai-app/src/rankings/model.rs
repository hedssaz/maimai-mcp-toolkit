use std::time::Duration;

use maimai_core::{AchievementRate, Difficulty, GroupId, QqId, SourceSongId};
use maimai_storage::{CachedB50Entry, CachedChart, RankingJob, RankingNamespace};
use time::OffsetDateTime;

use super::RankingError;
use crate::scores::ExactRatio;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B50Sort {
    Rating,
    FitIndex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SongSort {
    Achievements,
    Rating,
    DxScore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Rating,
    Detail,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RankWindow {
    pub limit: Option<usize>,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl RankWindow {
    pub fn validate(self) -> Result<Self, RankingError> {
        if self.limit == Some(0)
            || self.start == Some(0)
            || self.end == Some(0)
            || (self.start.is_some() != self.end.is_some())
            || self
                .start
                .zip(self.end)
                .is_some_and(|(start, end)| start > end)
        {
            return Err(RankingError::InvalidInput {
                field: "rank window",
            });
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaxConcurrency(usize);

impl MaxConcurrency {
    pub fn new(value: usize) -> Result<Self, RankingError> {
        if (1..=20).contains(&value) {
            Ok(Self(value))
        } else {
            Err(RankingError::InvalidInput {
                field: "maxConcurrency",
            })
        }
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

impl Default for MaxConcurrency {
    fn default() -> Self {
        Self(3)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaxMembers(usize);

impl MaxMembers {
    pub fn new(value: usize) -> Result<Self, RankingError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(RankingError::InvalidInput {
                field: "maxMembers",
            })
        }
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryDelay(Duration);

impl QueryDelay {
    pub const MAX: Duration = Duration::from_secs(10);

    pub fn new(value: Duration) -> Result<Self, RankingError> {
        if value > Self::MAX {
            return Err(RankingError::InvalidInput {
                field: "queryDelayMs",
            });
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> Duration {
        self.0
    }
}

impl Default for QueryDelay {
    fn default() -> Self {
        Self(Duration::from_millis(250))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextSize(u8);

impl ContextSize {
    pub fn new(value: u8) -> Result<Self, RankingError> {
        if value <= 10 {
            Ok(Self(value))
        } else {
            Err(RankingError::InvalidInput {
                field: "contextSize",
            })
        }
    }

    pub const fn get(self) -> usize {
        self.0 as usize
    }
}

impl Default for ContextSize {
    fn default() -> Self {
        Self(3)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefreshOptions {
    pub no_cache: bool,
    pub query_delay: QueryDelay,
    pub max_concurrency: MaxConcurrency,
    pub max_members: Option<MaxMembers>,
}

impl Default for RefreshOptions {
    fn default() -> Self {
        Self {
            no_cache: true,
            query_delay: QueryDelay::default(),
            max_concurrency: MaxConcurrency::default(),
            max_members: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheStatus {
    pub namespace: RankingNamespace,
    pub group_id: GroupId,
    pub exists: bool,
    pub fresh: bool,
    pub age_seconds: Option<i64>,
    pub fetched_at: Option<OffsetDateTime>,
    pub next_reset_at: OffsetDateTime,
    pub member_count: Option<u32>,
    pub success_count: Option<u32>,
    pub failure_count: Option<u32>,
    pub skipped_count: Option<u32>,
    pub cache_hit_count: Option<u32>,
    pub shared_fetch_count: Option<u32>,
    pub job: Option<RankingJob>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankingLaunch {
    pub started: bool,
    pub reason: maimai_storage::RankingRefreshReason,
    pub cache: CacheStatus,
    pub job: RankingJob,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RankingResponse<T> {
    Ready(T),
    Started(Box<RankingLaunch>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct B50ReportOptions {
    pub sort: B50Sort,
    pub order: SortOrder,
    pub output: OutputMode,
    pub rating_min: Option<u32>,
    pub rating_max: Option<u32>,
    pub fit_min: Option<ExactRatio>,
    pub fit_max: Option<ExactRatio>,
    pub window: RankWindow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B50Row {
    pub rank: usize,
    pub entry: CachedB50Entry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B50Report {
    pub cache: CacheStatus,
    pub options: B50ReportOptions,
    pub matched_count: usize,
    pub all_entries: Vec<CachedB50Entry>,
    pub rows: Vec<B50Row>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B50MemberRank {
    pub cache: CacheStatus,
    pub qq: QqId,
    pub found: bool,
    pub target: Option<CachedB50Entry>,
    pub rank_desc: Option<usize>,
    pub rank_asc: Option<usize>,
    pub total_ranked: usize,
    pub context: Vec<B50Row>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongTarget {
    pub title: String,
    pub ids: Vec<SourceSongId>,
    pub difficulty: Option<Difficulty>,
    pub deluxe: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongReportOptions {
    pub target: SongTarget,
    pub sort: SongSort,
    pub order: SortOrder,
    pub achievements_min: Option<AchievementRate>,
    pub achievements_max: Option<AchievementRate>,
    pub window: RankWindow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongRow {
    pub rank: usize,
    pub qq: QqId,
    pub member: maimai_storage::RankingMember,
    pub record: CachedChart,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongReport {
    pub cache: CacheStatus,
    pub target: SongTarget,
    pub sort: SongSort,
    pub order: SortOrder,
    pub matched_count: usize,
    pub rows: Vec<SongRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongMemberRank {
    pub cache: CacheStatus,
    pub qq: QqId,
    pub target: SongTarget,
    pub found: bool,
    pub row: Option<SongRow>,
    pub rank_desc: Option<usize>,
    pub rank_asc: Option<usize>,
    pub total_ranked: usize,
    pub context: Vec<SongRow>,
    pub reverse_context: Vec<SongRow>,
}
