use maimai_core::Difficulty;

use crate::RenderError;

pub const ACHIEVEMENT_DISTRIBUTION_LEN: usize = 14;
pub const FULL_COMBO_DISTRIBUTION_LEN: usize = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicGlobalStatsView {
    pub display_id: Option<u32>,
    pub title: String,
    pub difficulty: Difficulty,
    pub achievement_distribution: [u64; ACHIEVEMENT_DISTRIBUTION_LEN],
    pub full_combo_distribution: [u64; FULL_COMBO_DISTRIBUTION_LEN],
}

impl MusicGlobalStatsView {
    pub fn new(
        display_id: Option<u32>,
        title: impl Into<String>,
        difficulty: Difficulty,
        achievement_distribution: [u64; ACHIEVEMENT_DISTRIBUTION_LEN],
        full_combo_distribution: [u64; FULL_COMBO_DISTRIBUTION_LEN],
    ) -> Result<Self, RenderError> {
        let title = title.into();
        if title.chars().any(char::is_control) {
            return Err(RenderError::invalid(
                "music_global_stats.title",
                "must not contain control characters",
            ));
        }
        if difficulty == Difficulty::Utage {
            return Err(RenderError::invalid(
                "music_global_stats.difficulty",
                "utage is not part of the public global-stats contract",
            ));
        }
        Ok(Self {
            display_id,
            title,
            difficulty,
            achievement_distribution,
            full_combo_distribution,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicGlobalStatsRenderedPng {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub(crate) const fn difficulty_label(value: Difficulty) -> &'static str {
    match value {
        Difficulty::Basic => "Basic",
        Difficulty::Advanced => "Advanced",
        Difficulty::Expert => "Expert",
        Difficulty::Master => "Master",
        Difficulty::ReMaster => "Re:Master",
        Difficulty::Utage => "Utage",
    }
}
