use std::path::PathBuf;

use maimai_catalog::{PlateName, PlateServer};
use maimai_core::{AchievementRate, ScoreSource};
use maimai_render::{FullComboStatus, FullSyncStatus};

use crate::scores::Lookup;

use super::CompletionError;

pub const MAX_COMPLETION_BATCH_ITEMS: usize = 50;
pub const PROGRESS_PAGE_SIZE: usize = 80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AchievementTarget {
    Eighty,
    S,
    SPlus,
    Ss,
    SsPlus,
    Sss,
    SssPlus,
}

impl AchievementTarget {
    pub const fn threshold(self) -> u32 {
        match self {
            Self::Eighty => 800_000,
            Self::S => 970_000,
            Self::SPlus => 980_000,
            Self::Ss => 990_000,
            Self::SsPlus => 995_000,
            Self::Sss => 1_000_000,
            Self::SssPlus => 1_005_000,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Eighty => "者",
            Self::S => "S",
            Self::SPlus => "S+",
            Self::Ss => "SS",
            Self::SsPlus => "SS+",
            Self::Sss => "SSS",
            Self::SssPlus => "SSS+",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionTarget {
    Achievement(AchievementTarget),
    FullCombo(FullComboStatus),
    FullSync(FullSyncStatus),
    PlateExtreme,
    PlateGeneral,
    PlateGod,
    PlateDance,
}

impl CompletionTarget {
    pub const fn plate_general() -> Self {
        Self::PlateGeneral
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Achievement(value) => value.label(),
            Self::FullCombo(value) => value.label(),
            Self::FullSync(value) => value.label(),
            Self::PlateExtreme => "极",
            Self::PlateGeneral => "将",
            Self::PlateGod => "神",
            Self::PlateDance => "舞舞",
        }
    }

    pub(crate) fn completed(
        self,
        achievement: Option<AchievementRate>,
        combo: Option<FullComboStatus>,
        sync: Option<FullSyncStatus>,
    ) -> bool {
        match self {
            Self::Achievement(target) => {
                achievement.is_some_and(|value| value.ten_thousandths() >= target.threshold())
            }
            Self::FullCombo(target) => combo.is_some_and(|value| value >= target),
            Self::FullSync(target) => sync.is_some_and(|value| value >= target),
            Self::PlateExtreme => combo.is_some(),
            Self::PlateGeneral => {
                achievement.is_some_and(|value| value.ten_thousandths() >= 1_000_000)
            }
            Self::PlateGod => combo.is_some_and(|value| value >= FullComboStatus::AllPerfect),
            Self::PlateDance => sync.is_some_and(|value| value >= FullSyncStatus::FullSyncDeluxe),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProgressCategory {
    #[default]
    Overview,
    Completed,
    Unfinished,
    NotStarted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionCapabilities {
    pub allow_jp: bool,
    pub fixed_source: Option<ScoreSource>,
}

impl CompletionCapabilities {
    pub const fn main() -> Self {
        Self {
            allow_jp: true,
            fixed_source: None,
        }
    }

    pub const fn public() -> Self {
        Self {
            allow_jp: false,
            fixed_source: Some(ScoreSource::DivingFish),
        }
    }

    pub const fn is_public(self) -> bool {
        !self.allow_jp && matches!(self.fixed_source, Some(ScoreSource::DivingFish))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateSpec {
    pub version: PlateName,
    pub target: CompletionTarget,
    pub server: Option<PlateServer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionIdentity {
    pub lookup: Lookup,
    pub source: Option<ScoreSource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LevelProgressRequest {
    pub identity: CompletionIdentity,
    pub level: String,
    pub target: CompletionTarget,
    pub server: PlateServer,
    pub category: ProgressCategory,
    pub page: usize,
}

impl LevelProgressRequest {
    pub fn validate(&self) -> Result<(), CompletionError> {
        if self.level.trim().is_empty() || self.level.chars().any(char::is_control) {
            return Err(CompletionError::invalid("level 格式不正确"));
        }
        if self.page == 0 {
            return Err(CompletionError::invalid("page 必须大于 0"));
        }
        if self.server == PlateServer::Custom {
            return Err(CompletionError::invalid("等级进度不支持 custom server"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionImage {
    pub index: usize,
    pub version: String,
    pub target: String,
    pub server: PlateServer,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlateProgressOutput {
    Text(String),
    Image(CompletionImage),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionItemError {
    pub index: usize,
    pub label: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlateBatchResult {
    pub results: Vec<CompletionImage>,
    pub errors: Vec<CompletionItemError>,
    pub source: Option<ScoreSource>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlateProgressBatchResult {
    pub results: Vec<PlateProgressItem>,
    pub errors: Vec<CompletionItemError>,
    pub source: Option<ScoreSource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateProgressItem {
    pub index: usize,
    pub version: String,
    pub target: String,
    pub server: PlateServer,
    pub output: PlateProgressOutput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionResponse<T> {
    pub value: T,
    pub source: ScoreSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingTableRequest {
    pub identity: CompletionIdentity,
    pub level: String,
    pub mode: maimai_render::RatingTableMode,
}

impl RatingTableRequest {
    pub fn validate(&self) -> Result<(), CompletionError> {
        let level = self.level.trim();
        let valid = matches!(
            level,
            "7" | "7+"
                | "8"
                | "8+"
                | "9"
                | "9+"
                | "10"
                | "10+"
                | "11"
                | "11+"
                | "12"
                | "12+"
                | "13"
                | "13+"
                | "14"
                | "14+"
                | "15"
        );
        if !valid {
            return Err(CompletionError::invalid(
                "rating 必须是 7 到 15 的等级（+ 等级最高为 14+）",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingTableImage {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}
