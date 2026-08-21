use maimai_core::{
    AchievementRank, AchievementRate, ChartConstant, ChartGeneration, Difficulty,
    FullComboStatus as ScoreCombo, SongIdValue,
};

use crate::RenderError;

pub const MAX_COMPLETION_ITEMS: usize = 2_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FullComboStatus {
    FullCombo,
    FullComboPlus,
    AllPerfect,
    AllPerfectPlus,
}

impl FullComboStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FullCombo => "FC",
            Self::FullComboPlus => "FC+",
            Self::AllPerfect => "AP",
            Self::AllPerfectPlus => "AP+",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FullSyncStatus {
    FullSync,
    FullSyncPlus,
    FullSyncDeluxe,
    FullSyncDeluxePlus,
}

impl FullSyncStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FullSync => "FS",
            Self::FullSyncPlus => "FS+",
            Self::FullSyncDeluxe => "FSD",
            Self::FullSyncDeluxePlus => "FSD+",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionState {
    pub completed: bool,
    pub achievement: Option<AchievementRate>,
    pub combo: Option<FullComboStatus>,
    pub sync: Option<FullSyncStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateChartState {
    pub difficulty: Difficulty,
    pub state: CompletionState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateMemberView {
    pub cover_id: SongIdValue,
    pub image_name: Option<String>,
    pub title: String,
    pub generation: ChartGeneration,
    pub master_level: String,
    pub charts: Vec<PlateChartState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlateTableKind {
    China,
    Japan,
    Custom,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateTableView {
    pub kind: PlateTableKind,
    pub version: String,
    pub target: String,
    pub declared_song_count: usize,
    pub members: Vec<PlateMemberView>,
}

impl PlateTableView {
    pub fn validate(&self) -> Result<(), RenderError> {
        safe_text(&self.version, "plate.version", true)?;
        safe_text(&self.target, "plate.target", true)?;
        if self.members.len() > MAX_COMPLETION_ITEMS {
            return Err(RenderError::invalid(
                "plate.members",
                "contains too many songs",
            ));
        }
        for member in &self.members {
            safe_text(&member.title, "plate.member.title", true)?;
            safe_text(&member.master_level, "plate.member.master_level", false)?;
            if let Some(image_name) = &member.image_name {
                safe_text(image_name, "plate.member.image_name", false)?;
            }
            if member.charts.len() > 5 {
                return Err(RenderError::invalid(
                    "plate.member.charts",
                    "contains more than five difficulty states",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreCardCell {
    pub cover_id: SongIdValue,
    pub image_name: Option<String>,
    pub title: String,
    pub generation: ChartGeneration,
    pub difficulty: Difficulty,
    pub level: String,
    pub constant: Option<ChartConstant>,
    pub achievement: Option<AchievementRate>,
    pub dx_score: Option<u32>,
    pub max_dx_score: Option<u32>,
    pub rating: Option<u32>,
    pub grade: Option<String>,
    pub combo: Option<FullComboStatus>,
    pub sync: Option<FullSyncStatus>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressPage {
    Overview,
    Completed { page: usize, pages: usize },
    Unfinished { page: usize, pages: usize },
    NotStarted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LevelProgressView {
    pub level: String,
    pub target: String,
    pub page: ProgressPage,
    pub total: usize,
    pub remaining: usize,
    pub completed: Vec<ScoreCardCell>,
    pub unfinished: Vec<ScoreCardCell>,
    pub not_started: Vec<ScoreCardCell>,
}

impl LevelProgressView {
    pub fn validate(&self) -> Result<(), RenderError> {
        safe_text(&self.level, "progress.level", true)?;
        safe_text(&self.target, "progress.target", true)?;
        let count = self.completed.len() + self.unfinished.len() + self.not_started.len();
        if count > MAX_COMPLETION_ITEMS {
            return Err(RenderError::invalid(
                "progress.charts",
                "contains too many chart cells",
            ));
        }
        for cell in self
            .completed
            .iter()
            .chain(&self.unfinished)
            .chain(&self.not_started)
        {
            safe_text(&cell.title, "progress.chart.title", true)?;
            safe_text(&cell.level, "progress.chart.level", false)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionRenderedPng {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub placeholder_covers: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RatingTableMode {
    Achievement,
    FullCombo,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RatingStatistics {
    pub clear: usize,
    pub sync: usize,
    pub s: usize,
    pub sp: usize,
    pub ss: usize,
    pub ssp: usize,
    pub sss: usize,
    pub sssp: usize,
    pub fc: usize,
    pub fcp: usize,
    pub ap: usize,
    pub app: usize,
    pub fs: usize,
    pub fsp: usize,
    pub fsd: usize,
    pub fsdp: usize,
}

impl RatingStatistics {
    pub const fn values(self) -> [usize; 16] {
        [
            self.clear, self.sync, self.s, self.sp, self.ss, self.ssp, self.sss, self.sssp,
            self.fc, self.fcp, self.ap, self.app, self.fs, self.fsp, self.fsd, self.fsdp,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RatingAllClear {
    Achievement(AchievementRank),
    FullCombo(ScoreCombo),
}

impl RatingAllClear {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Achievement(value) => match value {
                AchievementRank::SssPlus => "SSS+ ALL CLEAR",
                AchievementRank::Sss => "SSS ALL CLEAR",
                AchievementRank::SsPlus => "SS+ ALL CLEAR",
                AchievementRank::Ss => "SS ALL CLEAR",
                AchievementRank::SPlus => "S+ ALL CLEAR",
                AchievementRank::S => "S ALL CLEAR",
                _ => "ALL CLEAR",
            },
            Self::FullCombo(value) => match value {
                ScoreCombo::FullCombo => "FC ALL CLEAR",
                ScoreCombo::FullComboPlus => "FC+ ALL CLEAR",
                ScoreCombo::AllPerfect => "AP ALL CLEAR",
                ScoreCombo::AllPerfectPlus => "AP+ ALL CLEAR",
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingScoreCell {
    pub cover_id: SongIdValue,
    pub image_name: Option<String>,
    pub title: String,
    pub generation: ChartGeneration,
    pub difficulty: Difficulty,
    pub achievement: Option<AchievementRate>,
    pub combo: Option<ScoreCombo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingConstantGroup {
    pub constant: Option<ChartConstant>,
    pub cells: Vec<RatingScoreCell>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingTableView {
    pub level: String,
    pub mode: RatingTableMode,
    pub total: usize,
    pub statistics: RatingStatistics,
    pub all_clear: Option<RatingAllClear>,
    pub groups: Vec<RatingConstantGroup>,
}

impl RatingTableView {
    pub fn validate(&self) -> Result<(), RenderError> {
        safe_text(&self.level, "rating.level", true)?;
        if self.total > MAX_COMPLETION_ITEMS || self.groups.len() > MAX_COMPLETION_ITEMS {
            return Err(RenderError::invalid(
                "rating.charts",
                "contains too many chart cells",
            ));
        }
        let mut count = 0_usize;
        for group in &self.groups {
            count = count.saturating_add(group.cells.len());
            for cell in &group.cells {
                safe_text(&cell.title, "rating.chart.title", true)?;
                if cell.difficulty == Difficulty::Utage
                    || matches!(
                        cell.generation,
                        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
                    )
                {
                    return Err(RenderError::invalid(
                        "rating.chart",
                        "must not contain Utage charts",
                    ));
                }
            }
        }
        if count != self.total || count > MAX_COMPLETION_ITEMS {
            return Err(RenderError::invalid(
                "rating.total",
                "must equal the number of chart cells",
            ));
        }
        Ok(())
    }
}

pub(crate) fn safe_text(
    value: &str,
    field: &'static str,
    required: bool,
) -> Result<(), RenderError> {
    if value.chars().any(char::is_control) || (required && value.trim().is_empty()) {
        return Err(RenderError::invalid(
            field,
            "must be non-empty when required and contain no control characters",
        ));
    }
    Ok(())
}
