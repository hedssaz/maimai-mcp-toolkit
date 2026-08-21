use serde::Deserialize;
use serde_json::Number;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Scalar {
    Text(String),
    Number(Number),
}

impl Scalar {
    pub(super) fn text(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Number(value) => value.to_string(),
        }
    }
}

#[derive(Default, Deserialize)]
pub(super) struct MusicInfoArgs {
    pub music_id: Option<Scalar>,
    #[serde(rename = "musicId")]
    pub music_id_camel: Option<Scalar>,
    pub id: Option<Scalar>,
    pub query: Option<Scalar>,
    pub title: Option<Scalar>,
    #[serde(rename = "songQuery")]
    pub song_query_camel: Option<Scalar>,
    pub song_query: Option<Scalar>,
    #[serde(rename = "songType")]
    pub song_type_camel: Option<String>,
    pub song_type: Option<String>,
    #[serde(rename = "chartType")]
    pub chart_type_camel: Option<String>,
    pub chart_type: Option<String>,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    pub image_name: Option<String>,
    #[serde(rename = "imageName")]
    pub image_name_camel: Option<String>,
    pub qq: Option<Scalar>,
    pub username: Option<Scalar>,
    #[serde(rename = "knownTitle")]
    pub known_title_camel: Option<String>,
    pub known_title: Option<String>,
    pub name: Option<String>,
    pub artist: Option<String>,
    pub genre: Option<String>,
    pub category: Option<String>,
    pub version: Option<String>,
    pub from: Option<String>,
    pub bpm: Option<Scalar>,
    pub is_new: Option<bool>,
    #[serde(rename = "isNew")]
    pub is_new_camel: Option<bool>,
    #[serde(rename = "resolvedSong")]
    pub resolved_song_camel: Option<ResolvedSongDto>,
    pub resolved_song: Option<ResolvedSongDto>,
}

#[derive(Default, Deserialize)]
pub(super) struct ResolvedSongDto {
    pub id: Option<Scalar>,
    pub source_id: Option<Scalar>,
    pub title: Option<String>,
    pub image_name: Option<String>,
    #[serde(rename = "imageName")]
    pub image_name_camel: Option<String>,
    #[serde(default)]
    pub available_chart_types: Vec<String>,
    #[serde(default)]
    pub matched_charts: Vec<ResolvedChartDto>,
}

#[derive(Deserialize)]
pub(super) struct ResolvedChartDto {
    pub chart_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum BatchItemDto {
    Object(Box<MusicInfoArgs>),
    Query(Scalar),
    Ignored(serde::de::IgnoredAny),
}

#[derive(Default, Deserialize)]
pub(super) struct MusicInfoBatchArgs {
    pub qq: Option<Scalar>,
    pub username: Option<Scalar>,
    #[serde(default)]
    pub items: Vec<BatchItemDto>,
    #[serde(default)]
    pub queries: Vec<Scalar>,
    pub query: Option<Scalar>,
    #[serde(rename = "songType")]
    pub song_type_camel: Option<String>,
    pub song_type: Option<String>,
}
