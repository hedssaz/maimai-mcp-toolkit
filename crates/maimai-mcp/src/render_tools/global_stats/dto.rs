use serde::Deserialize;

use super::super::dto::{MusicInfoArgs, Scalar};

#[derive(Default, Deserialize)]
pub(super) struct MusicGlobalStatsArgs {
    #[serde(flatten)]
    pub music: MusicInfoArgs,
    pub difficulty: Option<String>,
    pub diff: Option<String>,
    pub level_index: Option<Scalar>,
    #[serde(rename = "levelIndex")]
    pub level_index_camel: Option<Scalar>,
    pub difficulty_index: Option<Scalar>,
}
