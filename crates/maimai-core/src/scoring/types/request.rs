use std::str::FromStr;

use super::super::ScoringError;
use super::{JudgmentCounts, NoteTotals, SearchConstraints, note::clean_name};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreMode {
    Base,
    BreakBonus,
    OldScore,
    DxScore,
    OldAchievement,
    DxAchievement,
}

impl ScoreMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::BreakBonus => "break_bonus",
            Self::OldScore => "oldscore",
            Self::DxScore => "dxscore",
            Self::OldAchievement => "oldacc",
            Self::DxAchievement => "dxacc",
        }
    }

    pub const fn is_achievement(self) -> bool {
        matches!(self, Self::OldAchievement | Self::DxAchievement)
    }
}

impl FromStr for ScoreMode {
    type Err = ScoringError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match clean_name(value).as_str() {
            "base" => Ok(Self::Base),
            "break_bonus" => Ok(Self::BreakBonus),
            "oldscore" | "old_score" | "finale" => Ok(Self::OldScore),
            "dxscore" | "dx_score" | "dxstar" | "dxstars" | "star" | "stars" => Ok(Self::DxScore),
            "oldacc"
            | "old_acc"
            | "old_achievement"
            | "old_achievement_rate"
            | "old_percent"
            | "old_percentage" => Ok(Self::OldAchievement),
            "dxacc" | "dx_achievement" | "dx_acc" | "acc" => Ok(Self::DxAchievement),
            _ => Err(ScoringError::UnknownScoreMode(value.to_owned())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayMode {
    Floor,
    HalfUp,
    Exact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PercentageInput {
    /// 省略小数点的显示整数，例如四位小数时 `1004999` 表示 `100.4999`。
    Scaled(i128),
    Decimal(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchTarget {
    RawExact(i128),
    RawRange {
        min: Option<i128>,
        max: Option<i128>,
    },
    PercentageExact(PercentageInput),
    PercentageRange {
        min: Option<PercentageInput>,
        max: Option<PercentageInput>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreCountRequest {
    pub counts: JudgmentCounts,
    pub score_mode: Option<ScoreMode>,
    pub display_digits: u32,
    pub include_zero: bool,
}

impl Default for ScoreCountRequest {
    fn default() -> Self {
        Self {
            counts: JudgmentCounts::empty(),
            score_mode: None,
            display_digits: 4,
            include_zero: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchRequest {
    pub note_totals: NoteTotals,
    pub score_mode: ScoreMode,
    pub target: SearchTarget,
    pub constraints: SearchConstraints,
    pub max_solutions: usize,
    pub max_states: usize,
    pub display_digits: u32,
    pub display_mode: DisplayMode,
}

impl SearchRequest {
    pub fn raw(note_totals: NoteTotals, score_mode: ScoreMode, target: SearchTarget) -> Self {
        Self {
            note_totals,
            score_mode,
            target,
            constraints: SearchConstraints::default(),
            max_solutions: 10,
            max_states: 200_000,
            display_digits: 4,
            display_mode: DisplayMode::Floor,
        }
    }
}
