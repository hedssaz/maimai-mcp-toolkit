use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct RenderB50Args {
    pub qq: Option<String>,
    pub username: Option<String>,
    pub style: Option<String>,
    pub compute_from_records: bool,
    pub source: Option<String>,
    pub static_dir: Option<String>,
    pub cover_cache_dir: Option<String>,
    pub timeout_ms: Option<u64>,
    pub title: Option<String>,
}
