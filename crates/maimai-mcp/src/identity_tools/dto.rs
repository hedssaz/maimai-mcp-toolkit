use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct RefreshArgs {
    pub force_refresh: bool,
    pub napcat_base_url: Option<String>,
    pub no_cache: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub group_delay_ms: Option<u64>,
    pub max_groups: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyArgs {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResolveArgs {
    pub query: String,
    pub group_id: Option<String>,
    pub max_results: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetArgs {
    pub qq: String,
    pub group_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDto {
    pub code: String,
    pub message: String,
    pub status: Option<u16>,
    pub body: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsDto {
    pub friend_count: u64,
    pub group_count: u64,
    pub group_member_rows: u64,
    pub unique_users: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDto {
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub refresh_reason: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_groups: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_groups: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friend_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_users: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<StatsDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheDto {
    pub cache_exists: bool,
    pub fresh: bool,
    pub age_seconds: Option<i64>,
    pub daily_reset_hour_utc: u8,
    pub fetched_at: Option<String>,
    pub updated_at: Option<String>,
    pub stats: StatsDto,
    pub job: Option<JobDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub group_id: String,
    pub group_name: Option<String>,
    pub group_nickname: String,
    pub card: Option<String>,
    pub nickname: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityDto {
    pub qq: String,
    pub qq_nickname: Option<String>,
    pub friend_nickname: Option<String>,
    pub preferred_group: Option<GroupDto>,
    pub groups: Vec<GroupDto>,
    pub waterfish_nickname: Option<String>,
    pub waterfish_username: Option<String>,
    pub waterfish_rating: Option<u32>,
    pub is_friend: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDto {
    #[serde(flatten)]
    pub identity: IdentityDto,
    pub match_score: u16,
    pub matched_fields: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCacheDto {
    pub fetched_at: Option<String>,
    pub stats: StatsDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveDto {
    pub query: String,
    pub group_id: Option<String>,
    pub matches: Vec<MatchDto>,
    pub ambiguous: bool,
    pub cache: ResolveCacheDto,
}
