use maimai_core::{
    ChartConstant, ChartKey, FullComboStatus, FullSyncStatus, PlayAchievement, QqId, ScoreSource,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlayerProfile {
    pub qq: QqId,
    pub nickname: Option<String>,
    pub player_rating: Option<i64>,
    pub player_old_rating: Option<i64>,
    pub player_new_rating: Option<i64>,
    pub score_source: Option<ScoreSource>,
    pub source_detail: Option<String>,
    pub raw: Option<Value>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlayerRecord {
    pub qq: QqId,
    pub chart: ChartKey,
    pub title: String,
    pub level: Option<String>,
    pub level_label: Option<String>,
    pub ds: Option<ChartConstant>,
    pub achievements: Option<PlayAchievement>,
    pub dx_score: Option<i64>,
    pub fc: Option<FullComboStatus>,
    pub fs: Option<FullSyncStatus>,
    pub rate: Option<String>,
    pub ra: Option<i64>,
    pub version: Option<String>,
    pub is_new: bool,
    pub score_source: ScoreSource,
    pub source_detail: Option<String>,
    pub raw: Option<Value>,
    pub payload: Value,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FullScoreSnapshot {
    source: ScoreSource,
    fetched_at: OffsetDateTime,
    profile: PlayerProfile,
    records: Vec<PlayerRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullScoreSnapshotWriteOutcome {
    Written,
    StaleIgnored,
}

impl FullScoreSnapshot {
    pub(crate) const fn new(
        source: ScoreSource,
        fetched_at: OffsetDateTime,
        profile: PlayerProfile,
        records: Vec<PlayerRecord>,
    ) -> Self {
        Self {
            source,
            fetched_at,
            profile,
            records,
        }
    }

    pub const fn source(&self) -> ScoreSource {
        self.source
    }

    pub const fn fetched_at(&self) -> OffsetDateTime {
        self.fetched_at
    }

    pub const fn profile(&self) -> &PlayerProfile {
        &self.profile
    }

    pub fn records(&self) -> &[PlayerRecord] {
        &self.records
    }

    pub fn into_parts(self) -> (PlayerProfile, Vec<PlayerRecord>) {
        (self.profile, self.records)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyImportReport {
    pub already_applied: bool,
    pub imported: u64,
    pub skipped: u64,
}
