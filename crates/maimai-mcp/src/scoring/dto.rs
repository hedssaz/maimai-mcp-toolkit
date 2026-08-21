use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Number;

pub(super) type NestedCounts = HashMap<String, HashMap<String, u32>>;
pub(super) type RestrictionMap = HashMap<String, OneOrMany>;

#[derive(Debug, Default, Deserialize)]
pub(super) struct ScoreCountsArgs {
    #[serde(default)]
    pub(super) counts: NestedCounts,
    #[serde(default = "default_display_digits")]
    pub(super) display_digits: u32,
    #[serde(default)]
    pub(super) include_zero: bool,
    #[serde(default)]
    pub(super) score_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FindCombinationsArgs {
    pub(super) note_totals: HashMap<String, u32>,
    #[serde(default)]
    pub(super) target_score: Option<ScoreValue>,
    #[serde(default)]
    pub(super) target_acc: Option<ScoreValue>,
    #[serde(default)]
    pub(super) target_percent: Option<ScoreValue>,
    #[serde(default)]
    pub(super) target_percentage: Option<ScoreValue>,
    #[serde(default)]
    pub(super) target_dxacc: Option<ScoreValue>,
    #[serde(default)]
    pub(super) target_oldacc: Option<ScoreValue>,
    #[serde(default)]
    pub(super) min_score: Option<ScoreValue>,
    #[serde(default)]
    pub(super) max_score: Option<ScoreValue>,
    #[serde(default)]
    pub(super) min_acc: Option<ScoreValue>,
    #[serde(default)]
    pub(super) min_percent: Option<ScoreValue>,
    #[serde(default)]
    pub(super) min_percentage: Option<ScoreValue>,
    #[serde(default)]
    pub(super) max_acc: Option<ScoreValue>,
    #[serde(default)]
    pub(super) max_percent: Option<ScoreValue>,
    #[serde(default)]
    pub(super) max_percentage: Option<ScoreValue>,
    #[serde(default = "default_score_mode")]
    pub(super) score_mode: String,
    #[serde(default)]
    pub(super) allowed_judgments: RestrictionMap,
    #[serde(default)]
    pub(super) disallowed_judgments: RestrictionMap,
    #[serde(default)]
    pub(super) fixed_counts: NestedCounts,
    #[serde(default)]
    pub(super) min_counts: NestedCounts,
    #[serde(default)]
    pub(super) max_counts: NestedCounts,
    #[serde(default)]
    pub(super) no_miss_good: Option<bool>,
    #[serde(default)]
    pub(super) all_notes_no_miss_good: Option<bool>,
    #[serde(default)]
    pub(super) fc_plus_only: Option<bool>,
    #[serde(default)]
    pub(super) no_good_miss: Option<bool>,
    #[serde(default)]
    pub(super) break_max_perfect_or_below: Option<u32>,
    #[serde(default)]
    pub(super) break_max_perfect_or_lower: Option<u32>,
    #[serde(default)]
    pub(super) break_max_below_critical: Option<u32>,
    #[serde(default)]
    pub(super) max_break_below_critical: Option<u32>,
    #[serde(default)]
    pub(super) break_max_non_critical: Option<u32>,
    #[serde(default)]
    pub(super) max_break_non_critical: Option<u32>,
    #[serde(default = "default_max_solutions")]
    pub(super) max_solutions: usize,
    #[serde(default = "default_max_states")]
    pub(super) max_states: usize,
    #[serde(default = "default_display_mode")]
    pub(super) display_mode: String,
    #[serde(default = "default_display_digits")]
    pub(super) display_digits: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum ScoreValue {
    Integer(i128),
    Number(Number),
    Text(String),
}

impl ScoreValue {
    pub(super) fn text(&self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::Number(value) => value.to_string(),
            Self::Text(value) => value.trim().to_owned(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub(super) fn values(&self) -> &[String] {
        match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }
}

const fn default_display_digits() -> u32 {
    4
}

fn default_score_mode() -> String {
    "oldscore".to_owned()
}

const fn default_max_solutions() -> usize {
    10
}

const fn default_max_states() -> usize {
    200_000
}

fn default_display_mode() -> String {
    "floor".to_owned()
}
