use std::collections::BTreeMap;

use maimai_core::{
    ChartConstant, ChartGeneration, Difficulty, NoteCounts, SongIdValue, SourceSongId,
};

use crate::RenderError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicInfoChart {
    pub difficulty: Difficulty,
    pub level: String,
    pub constant: Option<ChartConstant>,
    pub fit_constant: Option<ChartConstant>,
    pub notes: Option<NoteCounts>,
    pub note_total: Option<u32>,
    pub charter: String,
}

impl MusicInfoChart {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        difficulty: Difficulty,
        level: impl Into<String>,
        constant: Option<ChartConstant>,
        fit_constant: Option<ChartConstant>,
        notes: Option<NoteCounts>,
        note_total: Option<u32>,
        charter: impl Into<String>,
    ) -> Result<Self, RenderError> {
        let level = safe_text(level.into(), "music.chart.level")?;
        let charter = safe_text(charter.into(), "music.chart.charter")?;
        Ok(Self {
            difficulty,
            level,
            constant,
            fit_constant,
            notes,
            note_total,
            charter,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerSongScoreContext {
    player_rating: Option<u32>,
    section_capacity: usize,
    section_count: usize,
    floor_rating: Option<u32>,
    chart_ratings: BTreeMap<Difficulty, u32>,
}

impl PlayerSongScoreContext {
    pub fn new(
        player_rating: Option<u32>,
        section_capacity: usize,
        section_count: usize,
        floor_rating: Option<u32>,
        chart_ratings: BTreeMap<Difficulty, u32>,
    ) -> Result<Self, RenderError> {
        if section_capacity == 0 || section_count > section_capacity {
            return Err(RenderError::invalid(
                "music.score_context",
                "section count must fit a non-zero section capacity",
            ));
        }
        if section_count == section_capacity && floor_rating.is_none() {
            return Err(RenderError::invalid(
                "music.score_context.floor_rating",
                "a full section requires its rating floor",
            ));
        }
        Ok(Self {
            player_rating,
            section_capacity,
            section_count,
            floor_rating,
            chart_ratings,
        })
    }

    pub const fn player_rating(&self) -> Option<u32> {
        self.player_rating
    }

    pub const fn section_capacity(&self) -> usize {
        self.section_capacity
    }

    pub const fn section_count(&self) -> usize {
        self.section_count
    }

    pub const fn floor_rating(&self) -> Option<u32> {
        self.floor_rating
    }

    pub fn chart_rating(&self, difficulty: Difficulty) -> Option<u32> {
        self.chart_ratings.get(&difficulty).copied()
    }

    pub(crate) fn gain(&self, difficulty: Difficulty, candidate: u32) -> u32 {
        if let Some(current) = self.chart_rating(difficulty) {
            return candidate.saturating_sub(current);
        }
        if self.section_count < self.section_capacity {
            return candidate;
        }
        let replaced = self.floor_rating.unwrap_or_default();
        candidate.saturating_sub(replaced)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicInfoView {
    pub song_id: Option<SourceSongId>,
    pub display_id: Option<u32>,
    pub title: String,
    pub artist: String,
    pub genre: String,
    pub version: String,
    pub bpm: Option<u32>,
    pub generation: Option<ChartGeneration>,
    pub is_new: bool,
    pub image_name: Option<String>,
    pub charts: Vec<MusicInfoChart>,
    pub score_context: Option<PlayerSongScoreContext>,
    pub require_cover: bool,
}

impl MusicInfoView {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        song_id: Option<SourceSongId>,
        display_id: Option<u32>,
        title: impl Into<String>,
        artist: impl Into<String>,
        genre: impl Into<String>,
        version: impl Into<String>,
        bpm: Option<u32>,
        generation: Option<ChartGeneration>,
        is_new: bool,
        image_name: Option<String>,
        charts: Vec<MusicInfoChart>,
    ) -> Result<Self, RenderError> {
        if charts.len() > 5 {
            return Err(RenderError::invalid(
                "music.charts",
                "contains more than five difficulty rows",
            ));
        }
        let mut rows = Vec::new();
        for chart in &charts {
            let row = difficulty_row(chart.difficulty);
            if rows.contains(&row) {
                return Err(RenderError::invalid(
                    "music.charts",
                    "contains duplicate difficulty rows",
                ));
            }
            rows.push(row);
        }
        Ok(Self {
            song_id,
            display_id,
            title: safe_text(title.into(), "music.title")?,
            artist: safe_text(artist.into(), "music.artist")?,
            genre: safe_text(genre.into(), "music.genre")?,
            version: safe_text(version.into(), "music.version")?,
            bpm,
            generation,
            is_new,
            image_name: image_name
                .map(|value| safe_text(value, "music.image_name"))
                .transpose()?
                .filter(|value| !value.is_empty()),
            charts,
            score_context: None,
            require_cover: false,
        })
    }

    pub fn with_score_context(mut self, context: Option<PlayerSongScoreContext>) -> Self {
        self.score_context = context;
        self
    }

    pub const fn require_cover(mut self, required: bool) -> Self {
        self.require_cover = required;
        self
    }

    pub(crate) fn cover_value(&self) -> Option<&SongIdValue> {
        self.song_id.as_ref().map(SourceSongId::value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicInfoRenderedPng {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub used_placeholder_cover: bool,
}

pub(crate) const fn difficulty_row(value: Difficulty) -> usize {
    match value {
        Difficulty::Basic | Difficulty::Utage => 0,
        Difficulty::Advanced => 1,
        Difficulty::Expert => 2,
        Difficulty::Master => 3,
        Difficulty::ReMaster => 4,
    }
}

fn safe_text(value: String, field: &'static str) -> Result<String, RenderError> {
    if value.chars().any(char::is_control) {
        return Err(RenderError::invalid(
            field,
            "must not contain control characters",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use maimai_core::Difficulty;

    use super::PlayerSongScoreContext;

    #[test]
    fn incomplete_section_replaces_existing_chart_before_filling_empty_slot()
    -> Result<(), Box<dyn std::error::Error>> {
        let context = PlayerSongScoreContext::new(
            Some(12_345),
            35,
            20,
            None,
            BTreeMap::from([(Difficulty::Master, 280)]),
        )?;
        assert_eq!(context.gain(Difficulty::Master, 300), 20);
        assert_eq!(context.gain(Difficulty::Expert, 240), 240);
        Ok(())
    }
}
