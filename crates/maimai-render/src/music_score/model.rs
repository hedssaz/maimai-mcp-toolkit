use maimai_core::{
    ChartConstant, ChartGeneration, Difficulty, FullComboStatus, FullSyncStatus, PlayAchievement,
};

use crate::{MusicInfoView, RenderError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicScoreRow {
    pub difficulty: Difficulty,
    pub level: String,
    pub constant: Option<ChartConstant>,
    pub note_total: u64,
    pub played: bool,
    pub achievement: Option<PlayAchievement>,
    pub grade: Option<String>,
    pub combo: Option<FullComboStatus>,
    pub sync: Option<FullSyncStatus>,
    pub dx_score: Option<u32>,
    pub rating: Option<u32>,
}

impl MusicScoreRow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        difficulty: Difficulty,
        level: impl Into<String>,
        constant: Option<ChartConstant>,
        note_total: u64,
        played: bool,
        achievement: Option<PlayAchievement>,
        grade: Option<String>,
        combo: Option<FullComboStatus>,
        sync: Option<FullSyncStatus>,
        dx_score: Option<u32>,
        rating: Option<u32>,
    ) -> Result<Self, RenderError> {
        let level = safe_text(level.into(), "music_score.level")?;
        let grade = grade
            .map(|value| safe_text(value, "music_score.grade"))
            .transpose()?
            .filter(|value| !value.is_empty());
        Ok(Self {
            difficulty,
            level,
            constant,
            note_total,
            played,
            achievement,
            grade,
            combo,
            sync,
            dx_score,
            rating,
        })
    }

    pub fn theoretical_dx_score(&self) -> Option<u64> {
        self.note_total.checked_mul(3).filter(|value| *value > 0)
    }

    pub fn stars(&self) -> Option<u8> {
        self.dx_score
            .zip(self.theoretical_dx_score())
            .and_then(|(score, theory)| crate::deluxe_score::star_level(u64::from(score), theory))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicScoreView {
    pub music: MusicInfoView,
    pub generation: ChartGeneration,
    pub rows: Vec<MusicScoreRow>,
}

impl MusicScoreView {
    pub fn new(
        music: MusicInfoView,
        generation: ChartGeneration,
        rows: Vec<MusicScoreRow>,
    ) -> Result<Self, RenderError> {
        let utage = is_utage(generation);
        if (utage && rows.len() > 1) || (!utage && rows.len() > 5) {
            return Err(RenderError::invalid(
                "music_score.rows",
                "row count does not fit the selected chart generation",
            ));
        }
        let mut seen = Vec::new();
        for row in &rows {
            if seen.contains(&row.difficulty)
                || (utage && row.difficulty != Difficulty::Utage)
                || (!utage && row.difficulty == Difficulty::Utage)
            {
                return Err(RenderError::invalid(
                    "music_score.rows",
                    "row difficulty does not fit the selected chart generation",
                ));
            }
            seen.push(row.difficulty);
        }
        Ok(Self {
            music,
            generation,
            rows,
        })
    }

    pub const fn is_utage(&self) -> bool {
        is_utage(self.generation)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicScoreRenderedPng {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub used_placeholder_cover: bool,
}

const fn is_utage(generation: ChartGeneration) -> bool {
    matches!(
        generation,
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
    )
}

fn safe_text(value: String, field: &'static str) -> Result<String, RenderError> {
    if value.chars().any(char::is_control) {
        Err(RenderError::invalid(field, "must not contain controls"))
    } else {
        Ok(value)
    }
}
