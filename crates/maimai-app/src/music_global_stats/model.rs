use std::path::PathBuf;

use maimai_core::Difficulty;

use crate::music_info::{MusicInfoChartType, MusicInfoRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MusicGlobalStatsDifficulty {
    Basic,
    Advanced,
    Expert,
    Master,
    ReMaster,
}

impl MusicGlobalStatsDifficulty {
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Basic),
            1 => Some(Self::Advanced),
            2 => Some(Self::Expert),
            3 => Some(Self::Master),
            4 => Some(Self::ReMaster),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Basic => 0,
            Self::Advanced => 1,
            Self::Expert => 2,
            Self::Master => 3,
            Self::ReMaster => 4,
        }
    }

    pub const fn difficulty(self) -> Difficulty {
        match self {
            Self::Basic => Difficulty::Basic,
            Self::Advanced => Difficulty::Advanced,
            Self::Expert => Difficulty::Expert,
            Self::Master => Difficulty::Master,
            Self::ReMaster => Difficulty::ReMaster,
        }
    }
}

pub struct MusicGlobalStatsRequest {
    pub music: MusicInfoRequest,
    pub difficulty: MusicGlobalStatsDifficulty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicGlobalStatsImage {
    pub index: usize,
    pub query: String,
    pub music_id: String,
    pub title: String,
    pub chart_type: MusicInfoChartType,
    pub level_index: usize,
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MusicGlobalStatsFailureKind {
    Input,
    Render,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicGlobalStatsItemError {
    pub index: usize,
    pub query: String,
    pub chart_type: Option<MusicInfoChartType>,
    pub kind: MusicGlobalStatsFailureKind,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MusicGlobalStatsBatchResult {
    pub images: Vec<MusicGlobalStatsImage>,
    pub errors: Vec<MusicGlobalStatsItemError>,
}
