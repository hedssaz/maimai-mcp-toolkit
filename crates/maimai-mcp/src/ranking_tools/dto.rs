use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RefreshArgs {
    pub napcat_base_url: Option<String>,
    pub no_cache: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub query_delay_ms: Option<u64>,
    pub max_concurrency: Option<usize>,
    pub batch_size: Option<usize>,
    pub max_members: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct B50ReportArgs {
    pub group_id: Option<String>,
    pub force_refresh: Option<bool>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub fit_index_min: Option<Value>,
    pub fit_index_max: Option<Value>,
    pub output_mode: Option<String>,
    pub rating_min: Option<u32>,
    pub rating_max: Option<u32>,
    pub output_limit: Option<usize>,
    pub start_rank: Option<usize>,
    pub end_rank: Option<usize>,
    pub napcat_base_url: Option<String>,
    pub no_cache: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub query_delay_ms: Option<u64>,
    pub max_concurrency: Option<usize>,
    pub batch_size: Option<usize>,
    pub max_members: Option<usize>,
}

impl B50ReportArgs {
    pub fn refresh(&self) -> RefreshArgs {
        RefreshArgs {
            napcat_base_url: self.napcat_base_url.clone(),
            no_cache: self.no_cache,
            timeout_ms: self.timeout_ms,
            query_delay_ms: self.query_delay_ms,
            max_concurrency: self.max_concurrency,
            batch_size: self.batch_size,
            max_members: self.max_members,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CacheArgs {
    pub group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct B50JobArgs {
    pub group_id: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub output_mode: Option<String>,
    pub rating_min: Option<u32>,
    pub rating_max: Option<u32>,
    pub fit_index_min: Option<Value>,
    pub fit_index_max: Option<Value>,
    pub output_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct B50MemberArgs {
    pub group_id: Option<String>,
    pub qq: Option<String>,
    pub target: Option<String>,
    pub force_refresh: Option<bool>,
    pub output_mode: Option<String>,
    pub context_size: Option<u8>,
    pub napcat_base_url: Option<String>,
    pub no_cache: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub query_delay_ms: Option<u64>,
    pub max_concurrency: Option<usize>,
    pub batch_size: Option<usize>,
    pub max_members: Option<usize>,
}

impl B50MemberArgs {
    pub fn refresh(&self) -> RefreshArgs {
        RefreshArgs {
            napcat_base_url: self.napcat_base_url.clone(),
            no_cache: self.no_cache,
            timeout_ms: self.timeout_ms,
            query_delay_ms: self.query_delay_ms,
            max_concurrency: self.max_concurrency,
            batch_size: self.batch_size,
            max_members: self.max_members,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct B50RankAtArgs {
    pub group_id: Option<String>,
    pub rank: Option<usize>,
    pub sort_order: Option<String>,
    pub output_mode: Option<String>,
    pub rating_min: Option<u32>,
    pub rating_max: Option<u32>,
    pub fit_index_min: Option<Value>,
    pub fit_index_max: Option<Value>,
    pub force_refresh: Option<bool>,
    pub napcat_base_url: Option<String>,
    pub no_cache: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub query_delay_ms: Option<u64>,
    pub max_concurrency: Option<usize>,
    pub batch_size: Option<usize>,
    pub max_members: Option<usize>,
}

impl B50RankAtArgs {
    pub fn refresh(&self) -> RefreshArgs {
        RefreshArgs {
            napcat_base_url: self.napcat_base_url.clone(),
            no_cache: self.no_cache,
            timeout_ms: self.timeout_ms,
            query_delay_ms: self.query_delay_ms,
            max_concurrency: self.max_concurrency,
            batch_size: self.batch_size,
            max_members: self.max_members,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SongReportArgs {
    pub group_id: Option<String>,
    pub song_query: Option<String>,
    pub music_id: Option<Value>,
    pub level_index: Option<u8>,
    pub song_type: Option<String>,
    pub sort_order: Option<String>,
    pub sort_by: Option<String>,
    pub output_limit: Option<usize>,
    pub start_rank: Option<usize>,
    pub end_rank: Option<usize>,
    pub achievements_min: Option<Value>,
    pub achievements_max: Option<Value>,
    pub force_refresh: Option<bool>,
    pub napcat_base_url: Option<String>,
    pub no_cache: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub query_delay_ms: Option<u64>,
    pub max_concurrency: Option<usize>,
    pub batch_size: Option<usize>,
    pub max_members: Option<usize>,
    pub search_limit: Option<usize>,
}

impl SongReportArgs {
    pub fn refresh(&self) -> RefreshArgs {
        RefreshArgs {
            napcat_base_url: self.napcat_base_url.clone(),
            no_cache: self.no_cache,
            timeout_ms: self.timeout_ms,
            query_delay_ms: self.query_delay_ms,
            max_concurrency: self.max_concurrency,
            batch_size: self.batch_size,
            max_members: self.max_members,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SongMemberArgs {
    pub group_id: Option<String>,
    pub qq: Option<String>,
    pub target: Option<String>,
    pub song_query: Option<String>,
    pub music_id: Option<Value>,
    pub level_index: Option<u8>,
    pub song_type: Option<String>,
    pub context_size: Option<u8>,
    pub force_refresh: Option<bool>,
    pub napcat_base_url: Option<String>,
    pub no_cache: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub query_delay_ms: Option<u64>,
    pub max_concurrency: Option<usize>,
    pub batch_size: Option<usize>,
    pub max_members: Option<usize>,
    pub search_limit: Option<usize>,
}

impl SongMemberArgs {
    pub fn refresh(&self) -> RefreshArgs {
        RefreshArgs {
            napcat_base_url: self.napcat_base_url.clone(),
            no_cache: self.no_cache,
            timeout_ms: self.timeout_ms,
            query_delay_ms: self.query_delay_ms,
            max_concurrency: self.max_concurrency,
            batch_size: self.batch_size,
            max_members: self.max_members,
        }
    }
}
