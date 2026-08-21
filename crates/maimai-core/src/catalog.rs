use serde::{Deserialize, Serialize};

use crate::{ChartConstant, ChartKey, SourceSongId};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NoteCounts {
    pub tap: u32,
    pub hold: u32,
    pub slide: u32,
    pub touch: u32,
    pub break_notes: u32,
}

impl NoteCounts {
    pub const fn total(self) -> u64 {
        self.tap as u64
            + self.hold as u64
            + self.slide as u64
            + self.touch as u64
            + self.break_notes as u64
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub key: ChartKey,
    pub source_ids: Vec<SourceSongId>,
    pub level: String,
    pub constant: Option<ChartConstant>,
    pub note_designer: String,
    pub notes: NoteCounts,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Music {
    pub primary_id: SourceSongId,
    pub source_ids: Vec<SourceSongId>,
    pub title: String,
    pub artist: String,
    pub genre: String,
    pub version: String,
    pub bpm: u32,
    pub aliases: Vec<String>,
    pub charts: Vec<Chart>,
}
