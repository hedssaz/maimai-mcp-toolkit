use std::path::PathBuf;

use maimai_core::ScoreSource;
use time::OffsetDateTime;

use crate::scores::Lookup;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RiseScoreAlgorithm {
    #[default]
    Legacy,
    Expected,
}

pub struct RiseScoreRequest {
    pub lookup: Lookup,
    pub source: Option<ScoreSource>,
    pub level: Option<String>,
    pub score: Option<u32>,
    pub algorithm: RiseScoreAlgorithm,
    pub now: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiseScoreImage {
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub source: ScoreSource,
    pub placeholder_covers: usize,
}
