use serde::{Deserialize, Serialize};
use serde_json::Number;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct SearchArgs {
    pub(super) query: Option<String>,
    pub(super) level: Option<String>,
    pub(super) genre: Option<String>,
    pub(super) version: Option<String>,
    pub(super) ds: Option<Scalar>,
    pub(super) ds_min: Option<Number>,
    pub(super) ds_max: Option<Number>,
    pub(super) fit_diff: Option<Scalar>,
    pub(super) fit_diff_min: Option<Number>,
    pub(super) fit_diff_max: Option<Number>,
    pub(super) fit_delta: Option<Scalar>,
    pub(super) fit_delta_min: Option<Number>,
    pub(super) fit_delta_max: Option<Number>,
    pub(super) fit_label: Option<String>,
    pub(super) region_has: Option<OneOrManyString>,
    pub(super) region_missing: Option<OneOrManyString>,
    pub(super) sort: Option<String>,
    pub(super) difficulty: Option<String>,
    pub(super) song_type: Option<String>,
    pub(super) is_new: Option<bool>,
    pub(super) is_new_source: Option<String>,
    pub(super) id: Option<Scalar>,
    pub(super) id_min: Option<u32>,
    pub(super) id_max: Option<u32>,
    pub(super) bpm: Option<Scalar>,
    pub(super) bpm_min: Option<Number>,
    pub(super) bpm_max: Option<Number>,
    pub(super) is_locked: Option<bool>,
    pub(super) artist: Option<String>,
    pub(super) charter: Option<String>,
    pub(super) tag: Option<OneOrManyScalar>,
    pub(super) tag_exclude: Option<OneOrManyScalar>,
    pub(super) released_after: Option<String>,
    pub(super) released_before: Option<String>,
    pub(super) limit: Option<usize>,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum Scalar {
    Number(Number),
    Text(String),
}

impl Scalar {
    pub(super) fn text(&self) -> String {
        match self {
            Self::Number(value) => value.to_string(),
            Self::Text(value) => value.trim().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum OneOrManyString {
    One(String),
    Many(Vec<String>),
}

impl OneOrManyString {
    pub(super) fn values(&self) -> &[String] {
        match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum OneOrManyScalar {
    One(Scalar),
    Many(Vec<Scalar>),
}

impl OneOrManyScalar {
    pub(super) fn values(&self) -> &[Scalar] {
        match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct BatchArgs {
    pub(super) items: Vec<BatchItem>,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct BatchItem {
    pub(super) key: Option<String>,
    #[serde(flatten)]
    pub(super) search: SearchArgs,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RandomArgs {
    #[serde(default = "default_count")]
    pub(super) count: usize,
    #[serde(default)]
    pub(super) seed: Option<Scalar>,
    #[serde(flatten)]
    pub(super) search: SearchArgs,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct TodayArgs {
    pub(super) qq: Scalar,
    #[serde(default)]
    pub(super) offset: Option<Scalar>,
    #[serde(default = "default_bot_name", rename = "botName")]
    pub(super) bot_name: String,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ListByIdArgs {
    #[serde(default = "default_order")]
    pub(super) order: String,
    #[serde(default = "default_list_limit")]
    pub(super) limit: usize,
    #[serde(flatten)]
    pub(super) search: SearchArgs,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct VersionsArgs {
    pub(super) query: Option<String>,
    pub(super) limit: Option<usize>,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct HistoryArgs {
    pub(super) query: String,
    pub(super) difficulty: Option<String>,
    pub(super) song_type: Option<String>,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct AliasMutationArgs {
    pub(super) kind: Option<String>,
    pub(super) song_id: Option<Scalar>,
    pub(super) title: Option<String>,
    pub(super) canonical: Option<String>,
    pub(super) alias: String,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct AliasListArgs {
    pub(super) kind: Option<String>,
    pub(super) query: Option<String>,
    pub(super) song_id: Option<Scalar>,
    pub(super) title: Option<String>,
    #[serde(default = "default_list_limit")]
    pub(super) limit: usize,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct RefreshArgs {
    pub(super) sources: Option<OneOrManyString>,
    pub(super) source: Option<String>,
    pub(super) force: Option<Boolish>,
    pub(super) ttl_days: Option<Scalar>,
    pub(super) source_ttl_days: Option<Scalar>,
    pub(super) check_only: Option<Boolish>,
    pub(super) timeout_seconds: Option<Scalar>,
    pub(super) background: Option<Boolish>,
    pub(super) bg: Option<Boolish>,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum Boolish {
    Bool(bool),
    Scalar(Scalar),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RefreshJobStatusArgs {
    #[serde(alias = "job_id")]
    pub(super) job_id: String,
    pub(super) format: Option<String>,
    pub(super) include_raw: Option<bool>,
    pub(super) debug: Option<bool>,
}

const fn default_count() -> usize {
    1
}

const fn default_list_limit() -> usize {
    20
}

fn default_order() -> String {
    "asc".to_owned()
}

fn default_bot_name() -> String {
    "铃".to_owned()
}
