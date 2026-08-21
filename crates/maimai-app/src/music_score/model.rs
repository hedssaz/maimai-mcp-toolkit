use std::path::PathBuf;

use maimai_core::ScoreSource;

use crate::{music_info::MusicInfoRequest, scores::Lookup};

pub struct MusicScoreRequest {
    pub music: MusicInfoRequest,
    pub player: Lookup,
    pub source: Option<ScoreSource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicScoreImage {
    pub index: usize,
    pub query: String,
    pub music_id: String,
    pub title: String,
    pub chart_type: String,
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicScoreItemError {
    pub index: usize,
    pub query: String,
    pub chart_type: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicScoreBatchResult {
    pub images: Vec<MusicScoreImage>,
    pub errors: Vec<MusicScoreItemError>,
    pub source: ScoreSource,
}
