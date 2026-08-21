use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct B50Args {
    pub qq: Option<String>,
    pub username: Option<String>,
    pub target: Option<String>,
    pub top_n: Option<u64>,
    pub section: Option<String>,
    pub include_raw: bool,
    pub timeout_ms: Option<u64>,
    pub include_chart_metadata: Option<bool>,
    pub group_id: Option<String>,
    pub source: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub level: Option<String>,
    pub difficulty: Option<String>,
    pub ds_min: Option<serde_json::Number>,
    pub ds_max: Option<serde_json::Number>,
    pub achievement_min: Option<serde_json::Number>,
    pub achievement_max: Option<serde_json::Number>,
    pub ra_min: Option<serde_json::Number>,
    pub ra_max: Option<serde_json::Number>,
    pub fit_diff_min: Option<serde_json::Number>,
    pub fit_diff_max: Option<serde_json::Number>,
    pub fit_delta_min: Option<serde_json::Number>,
    pub fit_delta_max: Option<serde_json::Number>,
    pub fit_label: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct BatchArgs {
    pub qqs: Vec<String>,
    pub top_n: Option<u64>,
    pub section: Option<String>,
    pub include_raw: bool,
    pub include_summaries: bool,
    pub timeout_ms: Option<u64>,
    pub include_chart_metadata: Option<bool>,
    pub query_delay_ms: Option<u64>,
    pub max_concurrency: Option<usize>,
    pub group_id: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub level: Option<String>,
    pub difficulty: Option<String>,
    pub ds_min: Option<serde_json::Number>,
    pub ds_max: Option<serde_json::Number>,
    pub achievement_min: Option<serde_json::Number>,
    pub achievement_max: Option<serde_json::Number>,
    pub ra_min: Option<serde_json::Number>,
    pub ra_max: Option<serde_json::Number>,
    pub fit_diff_min: Option<serde_json::Number>,
    pub fit_diff_max: Option<serde_json::Number>,
    pub fit_delta_min: Option<serde_json::Number>,
    pub fit_delta_max: Option<serde_json::Number>,
    pub fit_label: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct SongScoreArgs {
    pub qq: Option<String>,
    pub username: Option<String>,
    pub target: Option<String>,
    pub music_id: Option<Value>,
    pub song_query: Option<String>,
    pub difficulty: Option<String>,
    pub song_type: Option<String>,
    pub search_limit: Option<usize>,
    pub developer_token: Option<String>,
    pub source: Option<String>,
    pub include_raw: bool,
    pub timeout_ms: Option<u64>,
    pub group_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct RecordsArgs {
    pub qq: Option<String>,
    pub username: Option<String>,
    pub target: Option<String>,
    pub level: Option<String>,
    pub version: Option<Vec<String>>,
    pub plate: Option<String>,
    pub server: Option<String>,
    pub music_id: Option<Value>,
    pub source: Option<String>,
    pub include_raw: bool,
    pub timeout_ms: Option<u64>,
    pub group_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ListApiArgs {
    pub game: Option<String>,
    pub auth: Option<String>,
    pub include_mutating: bool,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct DivingFishApiArgs {
    pub operation: Option<String>,
    pub query: Option<Map<String, Value>>,
    pub body: Option<Value>,
    pub raw_body: Option<String>,
    pub developer_token: Option<String>,
    pub import_token: Option<String>,
    pub jwt_token: Option<String>,
    pub if_none_match: Option<String>,
    pub include_headers: bool,
    pub headers: Option<BTreeMap<String, String>>,
    pub confirm: Option<String>,
    pub timeout_ms: Option<u64>,
}
