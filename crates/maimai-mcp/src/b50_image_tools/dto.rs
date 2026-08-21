use maimai_app::b50_image::B50ImageStyle;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RenderArgs {
    pub qq: Option<String>,
    pub username: Option<String>,
    pub target: Option<String>,
    pub b50_data: Option<B50DataDto>,
    pub group_id: Option<String>,
    pub output_mode: Option<OutputMode>,
    pub style: Option<B50ImageStyle>,
    pub title: Option<String>,
    pub static_dir: Option<String>,
    pub timeout_ms: Option<u64>,
    pub output_dir: Option<String>,
    pub cover_cache_dir: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum OutputMode {
    #[default]
    File,
    Base64,
    Both,
}

impl OutputMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Base64 => "base64",
            Self::Both => "both",
        }
    }

    pub(super) const fn includes_base64(self) -> bool {
        matches!(self, Self::Base64 | Self::Both)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct B50DataDto {
    pub lookup: Option<LookupDto>,
    pub player: Option<PlayerDto>,
    pub counts: Option<CountsDto>,
    pub rating_breakdown: Option<RatingBreakdownDto>,
    #[serde(default)]
    pub charts: ChartsDto,
    pub source: Option<String>,
    pub source_preference: Option<SourcePreferenceDto>,
    pub requested_at: Option<String>,
    pub local_b50: Option<LocalB50Dto>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LookupDto {
    pub qq: Option<String>,
    pub username: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlayerDto {
    pub nickname: Option<String>,
    pub rating: Option<u32>,
    pub plate: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct CountsDto {
    #[serde(default)]
    pub sd: u32,
    #[serde(default)]
    pub dx: u32,
    #[serde(default)]
    pub total: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct RatingBreakdownDto {
    #[serde(default)]
    pub sd: u32,
    #[serde(default)]
    pub dx: u32,
    #[serde(default)]
    pub total: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct ChartsDto {
    #[serde(default)]
    pub sd: Vec<ChartDto>,
    #[serde(default)]
    pub dx: Vec<ChartDto>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChartDto {
    pub song_id: Option<SongIdDto>,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub chart_type: Option<String>,
    pub level: Option<String>,
    pub level_label: Option<String>,
    pub level_index: Option<u8>,
    pub ds: Option<ExactNumber>,
    pub achievements: Option<ExactNumber>,
    #[serde(default)]
    pub ra: u32,
    pub rate: Option<String>,
    pub fc: Option<String>,
    pub fs: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum SongIdDto {
    Number(Number),
    Text(String),
}

#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub(super) struct ExactNumber(Number);

impl ExactNumber {
    pub(super) fn text(&self) -> String {
        self.0.to_string()
    }
}

impl<'de> Deserialize<'de> for ExactNumber {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Number::deserialize(deserializer).map(Self)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SourcePreferenceDto {
    pub used_source: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LocalB50Dto {
    pub computed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetStyleArgs {
    pub style: B50ImageStyle,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyArgs {}
