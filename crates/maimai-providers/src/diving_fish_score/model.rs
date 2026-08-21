use maimai_core::{
    ChartConstant, ChartGeneration, Difficulty, FullComboStatus, FullSyncStatus, PlayAchievement,
    PlayerSelector, PlayerUsername, RatingBreakdown, SourceSongId,
};

use super::DivingFishScoreError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DivingFishChartGeneration {
    Standard,
    Deluxe,
    Utage,
}

impl DivingFishChartGeneration {
    pub const fn ranked_generation(self) -> Option<ChartGeneration> {
        match self {
            Self::Standard => Some(ChartGeneration::Standard),
            Self::Deluxe => Some(ChartGeneration::Deluxe),
            Self::Utage => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DivingFishPlayer {
    pub nickname: Option<String>,
    pub username: Option<PlayerUsername>,
    pub rating: Option<u32>,
    pub additional_rating: Option<u32>,
    pub plate: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DivingFishScore {
    pub song_id: SourceSongId,
    pub title: String,
    pub generation: DivingFishChartGeneration,
    pub difficulty: Difficulty,
    pub level: String,
    pub constant: Option<ChartConstant>,
    pub achievements: Option<PlayAchievement>,
    pub dx_score: Option<u32>,
    pub rating: Option<u32>,
    pub grade: Option<String>,
    pub full_combo: Option<FullComboStatus>,
    pub full_sync: Option<FullSyncStatus>,
    pub version: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DivingFishScoreCounts {
    pub sd: usize,
    pub dx: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DivingFishB50 {
    pub lookup: PlayerSelector,
    pub player: DivingFishPlayer,
    pub counts: DivingFishScoreCounts,
    pub rating_breakdown: RatingBreakdown,
    pub sd: Vec<DivingFishScore>,
    pub dx: Vec<DivingFishScore>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DivingFishPlayerRecords {
    pub lookup: PlayerSelector,
    pub player: DivingFishPlayer,
    pub records: Vec<DivingFishScore>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DivingFishRatingEntry {
    pub username: PlayerUsername,
    pub rating: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateVersions(Vec<String>);

impl PlateVersions {
    pub fn new(values: impl IntoIterator<Item = String>) -> Result<Self, DivingFishScoreError> {
        let mut output = Vec::new();
        for value in values {
            if value.chars().any(char::is_control) {
                return Err(DivingFishScoreError::invalid_request(
                    "plate version 不能包含控制字符",
                ));
            }
            let value = value.trim().to_owned();
            if value.is_empty() {
                return Err(DivingFishScoreError::invalid_request(
                    "plate version 不能为空",
                ));
            }
            if !output.contains(&value) {
                output.push(value);
            }
        }
        if output.is_empty() {
            return Err(DivingFishScoreError::invalid_request(
                "query_plate 至少需要一个 version",
            ));
        }
        Ok(Self(output))
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}
