use maimai_core::{
    AchievementRate, ChartConstant, ChartKey, FullComboStatus, FullSyncStatus, GroupId, QqId,
    RatingBreakdown,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RankingNamespace {
    B50,
    SongScore,
}

impl RankingNamespace {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::B50 => "b50",
            Self::SongScore => "song_score",
        }
    }

    pub(super) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "b50" => Some(Self::B50),
            "song_score" => Some(Self::SongScore),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RankingRefreshReason {
    Miss,
    Stale,
    ForceRefresh,
}

impl RankingRefreshReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Miss => "miss",
            Self::Stale => "stale",
            Self::ForceRefresh => "force_refresh",
        }
    }

    pub(super) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "miss" => Some(Self::Miss),
            "stale" => Some(Self::Stale),
            "force_refresh" => Some(Self::ForceRefresh),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankingMember {
    pub ordinal: u32,
    pub qq: QqId,
    pub nickname: Option<String>,
    pub card: Option<String>,
    pub display_name: String,
    pub waterfish_nickname: Option<String>,
    pub waterfish_username: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CachedPlayer {
    pub nickname: Option<String>,
    pub username: Option<String>,
    pub rating: Option<u32>,
    pub actual_rating: Option<u32>,
    pub additional_rating: Option<u32>,
    pub plate: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CachedChart {
    pub key: ChartKey,
    pub title: String,
    pub level: String,
    pub constant: Option<ChartConstant>,
    pub achievements: Option<AchievementRate>,
    pub dx_score: Option<u32>,
    pub rating: Option<u32>,
    pub original_rating: Option<u32>,
    pub grade: Option<String>,
    pub full_combo: Option<FullComboStatus>,
    pub full_sync: Option<FullSyncStatus>,
    pub version: String,
    pub is_current: bool,
    pub fit_constant: Option<ChartConstant>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CachedB50Chart {
    pub section: B50Section,
    pub ordinal: u32,
    pub chart: CachedChart,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum B50Section {
    B35,
    B15,
}

impl B50Section {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::B35 => "b35",
            Self::B15 => "b15",
        }
    }

    pub(super) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "b35" => Some(Self::B35),
            "b15" => Some(Self::B15),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachedB50Entry {
    pub member: RankingMember,
    pub player: CachedPlayer,
    pub rating_breakdown: RatingBreakdown,
    pub fit_index: CachedFitIndex,
    pub charts: Vec<CachedB50Chart>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CachedFitIndexLabel {
    ClearlyInflated,
    SlightlyInflated,
    Balanced,
    SlightlyDeflated,
    ClearlyDeflated,
}

impl CachedFitIndexLabel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClearlyInflated => "clearly_inflated",
            Self::SlightlyInflated => "slightly_inflated",
            Self::Balanced => "balanced",
            Self::SlightlyDeflated => "slightly_deflated",
            Self::ClearlyDeflated => "clearly_deflated",
        }
    }

    pub(super) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "clearly_inflated" => Some(Self::ClearlyInflated),
            "slightly_inflated" => Some(Self::SlightlyInflated),
            "balanced" => Some(Self::Balanced),
            "slightly_deflated" => Some(Self::SlightlyDeflated),
            "clearly_deflated" => Some(Self::ClearlyDeflated),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CachedExactRatio {
    pub numerator: i128,
    pub denominator: u128,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CachedFitIndexSection {
    pub virtual_rating: Option<i64>,
    pub virtual_ratio_percent: Option<CachedExactRatio>,
    pub weighted_average_delta: Option<CachedExactRatio>,
    pub counted: u32,
    pub missing: u32,
    pub total_rating: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CachedFitIndex {
    pub label: Option<CachedFitIndexLabel>,
    pub b50: CachedFitIndexSection,
    pub b35: CachedFitIndexSection,
    pub b15: CachedFitIndexSection,
}

impl CachedFitIndex {
    pub const fn available(self) -> bool {
        self.b50.counted > 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankingSnapshot {
    pub namespace: RankingNamespace,
    pub group_id: GroupId,
    pub generation: u64,
    pub fetched_at: OffsetDateTime,
    pub next_reset_at: OffsetDateTime,
    pub member_count: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub skipped_count: u32,
    pub cache_hit_count: u32,
    pub shared_fetch_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RankingSnapshotData {
    B50(Vec<CachedB50Entry>),
    SongScores {
        members: Vec<RankingMember>,
        records: Vec<(QqId, CachedChart)>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankingCache {
    pub snapshot: RankingSnapshot,
    pub data: RankingSnapshotData,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RankingJobStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl RankingJobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub(super) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RankingJobErrorCode {
    InvalidInput,
    Timeout,
    Network,
    Http,
    Provider,
    Storage,
    Catalog,
    Unknown,
}

impl RankingJobErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "INVALID_INPUT",
            Self::Timeout => "TIMEOUT",
            Self::Network => "NETWORK_ERROR",
            Self::Http => "HTTP_ERROR",
            Self::Provider => "PROVIDER_ERROR",
            Self::Storage => "STORAGE_ERROR",
            Self::Catalog => "CATALOG_ERROR",
            Self::Unknown => "UNKNOWN_ERROR",
        }
    }

    pub(super) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "INVALID_INPUT" => Some(Self::InvalidInput),
            "TIMEOUT" => Some(Self::Timeout),
            "NETWORK_ERROR" => Some(Self::Network),
            "HTTP_ERROR" => Some(Self::Http),
            "PROVIDER_ERROR" => Some(Self::Provider),
            "STORAGE_ERROR" => Some(Self::Storage),
            "CATALOG_ERROR" => Some(Self::Catalog),
            "UNKNOWN_ERROR" => Some(Self::Unknown),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankingJobError {
    pub code: RankingJobErrorCode,
    pub message: String,
    pub status: Option<u16>,
    pub body: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RankingJobProgress {
    pub processed_count: u32,
    pub total_count: Option<u32>,
    pub cached_count: u32,
    pub skipped_count: u32,
    pub transient_failure_count: u32,
    pub current_qq: Option<QqId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankingJob {
    pub namespace: RankingNamespace,
    pub group_id: GroupId,
    pub generation: u64,
    pub status: RankingJobStatus,
    pub started_at: OffsetDateTime,
    pub finished_at: Option<OffsetDateTime>,
    pub refresh_reason: RankingRefreshReason,
    pub message: String,
    pub progress: RankingJobProgress,
    pub member_count: Option<u32>,
    pub success_count: Option<u32>,
    pub skipped_count: Option<u32>,
    pub error: Option<RankingJobError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RankingJobStart {
    Started(RankingJob),
    AlreadyRunning(RankingJob),
}
