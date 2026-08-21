use serde::Deserialize;

use super::super::dto::MusicInfoArgs;

#[derive(Default, Deserialize)]
pub(super) struct MusicScoreArgs {
    #[serde(flatten)]
    pub music: MusicInfoArgs,
    #[serde(rename = "scoreSource")]
    pub score_source_camel: Option<String>,
    pub score_source: Option<String>,
    #[serde(rename = "dataSource")]
    pub data_source_camel: Option<String>,
    pub data_source: Option<String>,
    pub source: Option<String>,
}
