use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, SongIdValue,
};

use crate::RenderError;

pub const RISE_SCORE_LIMIT: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiseScoreSection {
    LegacyB35,
    CurrentB15,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiseScoreCandidate {
    pub key: ChartKey,
    pub display_id: SongIdValue,
    pub cover_id: SongIdValue,
    pub image_name: Option<String>,
    pub title: String,
    pub constant: ChartConstant,
    pub old_achievement: Option<AchievementRate>,
    pub old_rating: u32,
    pub target_achievement: AchievementRate,
    pub target_rating: u32,
    pub gain: u32,
}

impl RiseScoreCandidate {
    pub const fn generation(&self) -> ChartGeneration {
        self.key.generation()
    }

    pub const fn difficulty(&self) -> Difficulty {
        self.key.difficulty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiseScoreView {
    pub legacy: Vec<RiseScoreCandidate>,
    pub legacy_replacement_floor: u32,
    pub current: Vec<RiseScoreCandidate>,
    pub current_replacement_floor: u32,
}

impl RiseScoreView {
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.legacy.is_empty() && self.current.is_empty() {
            return Err(RenderError::invalid(
                "rise_score.items",
                "at least one recommendation is required",
            ));
        }
        if self.legacy.len() > RISE_SCORE_LIMIT || self.current.len() > RISE_SCORE_LIMIT {
            return Err(RenderError::invalid(
                "rise_score.items",
                "each section supports at most five recommendations",
            ));
        }
        for item in self.legacy.iter().chain(&self.current) {
            if item.title.chars().any(char::is_control) || item.title.chars().count() > 256 {
                return Err(RenderError::invalid(
                    "rise_score.item.title",
                    "must contain at most 256 non-control characters",
                ));
            }
            if matches!(item.difficulty(), Difficulty::Utage)
                || matches!(
                    item.generation(),
                    ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
                )
            {
                return Err(RenderError::invalid(
                    "rise_score.item.chart",
                    "utage charts are unsupported",
                ));
            }
            if !matches!(
                item.target_achievement.ten_thousandths(),
                990_000 | 995_000 | 1_000_000 | 1_005_000
            ) {
                return Err(RenderError::invalid(
                    "rise_score.item.target_achievement",
                    "must be 99, 99.5, 100, or 100.5",
                ));
            }
        }
        Ok(())
    }

    pub fn height(&self) -> u32 {
        height_for_rows(self.legacy.len().max(self.current.len()))
    }
}

pub(super) fn height_for_rows(rows: usize) -> u32 {
    u32::try_from(rows)
        .unwrap_or(5)
        .saturating_mul(140)
        .saturating_add(260)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiseScoreRenderedPng {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub placeholder_covers: usize,
}
