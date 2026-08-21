use std::{fmt, path::PathBuf};

use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, Difficulty as CoreDifficulty, RatingBreakdown,
    SourceSongId,
};

use crate::RenderError;

pub const MAX_B35_CARDS: usize = 35;
pub const MAX_B15_CARDS: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartType {
    Standard,
    Deluxe,
}

impl fmt::Display for ChartType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Standard => "SD",
            Self::Deluxe => "DX",
        })
    }
}

impl TryFrom<ChartGeneration> for ChartType {
    type Error = RenderError;

    fn try_from(value: ChartGeneration) -> Result<Self, Self::Error> {
        match value {
            ChartGeneration::Standard => Ok(Self::Standard),
            ChartGeneration::Deluxe => Ok(Self::Deluxe),
            ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer => {
                Err(RenderError::invalid(
                    "score.chart_type",
                    "legacy B50 does not support Utage charts",
                ))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Basic,
    Advanced,
    Expert,
    Master,
    ReMaster,
}

impl fmt::Display for Difficulty {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Basic => "Basic",
            Self::Advanced => "Advanced",
            Self::Expert => "Expert",
            Self::Master => "Master",
            Self::ReMaster => "Re:MASTER",
        })
    }
}

impl TryFrom<CoreDifficulty> for Difficulty {
    type Error = RenderError;

    fn try_from(value: CoreDifficulty) -> Result<Self, Self::Error> {
        match value {
            CoreDifficulty::Basic => Ok(Self::Basic),
            CoreDifficulty::Advanced => Ok(Self::Advanced),
            CoreDifficulty::Expert => Ok(Self::Expert),
            CoreDifficulty::Master => Ok(Self::Master),
            CoreDifficulty::ReMaster => Ok(Self::ReMaster),
            CoreDifficulty::Utage => Err(RenderError::invalid(
                "score.difficulty",
                "legacy B50 does not support Utage difficulty",
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerHeader {
    nickname: String,
    rating: Option<u32>,
    plate: Option<String>,
}

impl PlayerHeader {
    pub fn new(
        nickname: impl Into<String>,
        rating: Option<u32>,
        plate: Option<String>,
    ) -> Result<Self, RenderError> {
        let nickname = required_text(nickname.into(), "player.nickname")?;
        Ok(Self {
            nickname,
            rating,
            plate: optional_text(plate, "player.plate")?,
        })
    }

    pub fn nickname(&self) -> &str {
        &self.nickname
    }

    pub fn rating(&self) -> Option<u32> {
        self.rating
    }

    pub fn plate(&self) -> Option<&str> {
        self.plate.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoreCard {
    song_id: Option<SourceSongId>,
    title: String,
    chart_type: ChartType,
    difficulty: Difficulty,
    level: String,
    constant: Option<ChartConstant>,
    achievements: Option<AchievementRate>,
    rating: u32,
    grade: Option<String>,
    combo: Option<String>,
    sync: Option<String>,
    cover_path: Option<PathBuf>,
}

impl ScoreCard {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        song_id: Option<SourceSongId>,
        title: impl Into<String>,
        chart_type: ChartType,
        difficulty: Difficulty,
        level: impl Into<String>,
        constant: Option<ChartConstant>,
        achievements: Option<AchievementRate>,
        rating: u32,
    ) -> Result<Self, RenderError> {
        let title = required_text(title.into(), "score.title")?;
        let level = required_text(level.into(), "score.level")?;
        Ok(Self {
            song_id,
            title,
            chart_type,
            difficulty,
            level,
            constant,
            achievements,
            rating,
            grade: None,
            combo: None,
            sync: None,
            cover_path: None,
        })
    }

    pub fn with_markers(
        mut self,
        grade: Option<String>,
        combo: Option<String>,
        sync: Option<String>,
    ) -> Result<Self, RenderError> {
        self.grade = optional_text(grade, "score.grade")?;
        self.combo = optional_text(combo, "score.combo")?;
        self.sync = optional_text(sync, "score.sync")?;
        Ok(self)
    }

    pub fn with_cover(mut self, path: impl Into<PathBuf>) -> Self {
        self.cover_path = Some(path.into());
        self
    }

    pub fn song_id(&self) -> Option<&SourceSongId> {
        self.song_id.as_ref()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn chart_type(&self) -> ChartType {
        self.chart_type
    }

    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    pub fn level(&self) -> &str {
        &self.level
    }

    pub fn constant(&self) -> Option<ChartConstant> {
        self.constant
    }

    pub fn achievements(&self) -> Option<AchievementRate> {
        self.achievements
    }

    pub fn rating(&self) -> u32 {
        self.rating
    }

    pub fn grade(&self) -> Option<&str> {
        self.grade.as_deref()
    }

    pub fn combo(&self) -> Option<&str> {
        self.combo.as_deref()
    }

    pub fn sync(&self) -> Option<&str> {
        self.sync.as_deref()
    }

    pub fn cover_path(&self) -> Option<&std::path::Path> {
        self.cover_path.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct B50View {
    title: String,
    player: PlayerHeader,
    breakdown: RatingBreakdown,
    b35: Vec<ScoreCard>,
    b15: Vec<ScoreCard>,
}

impl B50View {
    pub fn new(
        title: impl Into<String>,
        player: PlayerHeader,
        breakdown: RatingBreakdown,
        b35: Vec<ScoreCard>,
        b15: Vec<ScoreCard>,
    ) -> Result<Self, RenderError> {
        let title = required_text(title.into(), "title")?;
        if b35.len() > MAX_B35_CARDS {
            return Err(RenderError::invalid("b35", "contains more than 35 cards"));
        }
        if b15.len() > MAX_B15_CARDS {
            return Err(RenderError::invalid("b15", "contains more than 15 cards"));
        }
        Ok(Self {
            title,
            player,
            breakdown,
            b35,
            b15,
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn player(&self) -> &PlayerHeader {
        &self.player
    }

    pub fn breakdown(&self) -> RatingBreakdown {
        self.breakdown
    }

    pub fn b35(&self) -> &[ScoreCard] {
        &self.b35
    }

    pub fn b15(&self) -> &[ScoreCard] {
        &self.b15
    }

    pub fn card_count(&self) -> usize {
        self.b35.len() + self.b15.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreSection {
    B35,
    B15,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingCoverReason {
    NotProvided,
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingCover {
    pub section: ScoreSection,
    pub index: usize,
    pub song_id: Option<SourceSongId>,
    pub title: String,
    pub reason: MissingCoverReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderMetadata {
    pub width: u32,
    pub height: u32,
    pub card_count: usize,
    pub missing_covers: Vec<MissingCover>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedPng {
    pub bytes: Vec<u8>,
    pub metadata: RenderMetadata,
}

fn required_text(value: String, field: &'static str) -> Result<String, RenderError> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(RenderError::invalid(
            field,
            "must be non-empty and contain no control characters",
        ));
    }
    Ok(value)
}

fn optional_text(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<String>, RenderError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().any(char::is_control) {
        return Err(RenderError::invalid(
            field,
            "must contain no control characters",
        ));
    }
    Ok(Some(value))
}
