use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct QueryScoreBySongArgs {
    pub qq: Option<String>,
    pub username: Option<String>,
    pub target: Option<String>,
    pub song_query: Option<String>,
    pub group_id: Option<String>,
    pub difficulty: Option<String>,
    pub song_type: Option<String>,
    pub search_limit: Option<usize>,
    pub timeout_ms: Option<u64>,
    pub include_raw: bool,
}
