use maimai_core::{PlayAchievement, PlayAchievementKind};
use serde::Serialize;

use super::{
    CollectionRef, FriendCode, FullCombo, FullSync, LxnsChartType, LxnsDifficulty, LxnsScoreError,
    LxnsScoreErrorCode, LxnsSongId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlayerUpdate {
    pub name: String,
    pub rating: u32,
    pub friend_code: FriendCode,
    pub course_rank: u32,
    pub class_rank: u32,
    pub star: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trophy: Option<CollectionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<CollectionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_plate: Option<CollectionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<CollectionRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScoreUpload {
    id: LxnsSongId,
    #[serde(rename = "type")]
    chart_type: LxnsChartType,
    level_index: LxnsDifficulty,
    #[serde(with = "super::codec::achievement_percent")]
    achievements: PlayAchievement,
    fc: Option<FullCombo>,
    fs: Option<FullSync>,
    dx_score: u32,
}

impl ScoreUpload {
    pub fn new(
        id: LxnsSongId,
        chart_type: LxnsChartType,
        level_index: LxnsDifficulty,
        achievements: PlayAchievement,
        fc: Option<FullCombo>,
        fs: Option<FullSync>,
        dx_score: u32,
    ) -> Result<Self, LxnsScoreError> {
        let expected_kind = match chart_type {
            LxnsChartType::Standard | LxnsChartType::Deluxe => PlayAchievementKind::Ranked,
            LxnsChartType::Utage => PlayAchievementKind::Utage,
        };
        if achievements.kind() != expected_kind {
            return Err(invalid("LXNS score upload chart type 与达成率类型不匹配"));
        }
        Ok(Self {
            id,
            chart_type,
            level_index,
            achievements,
            fc,
            fs,
            dx_score,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadReceipt {
    pub uploaded: usize,
    pub updated: Option<u64>,
}

fn invalid(message: &'static str) -> LxnsScoreError {
    LxnsScoreError::new(LxnsScoreErrorCode::InvalidRequest, message)
}
