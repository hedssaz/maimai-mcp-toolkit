use std::collections::BTreeSet;

use maimai_core::{
    Chart, ChartConstant, ChartGeneration, Difficulty, Music, SongIdValue, SourceSongId,
};
use rust_decimal::Decimal;
use thiserror::Error;
use time::Date;

use crate::{
    ChartQueryMetadata, Region, SongQueryMetadata, SourceChartProjection, SourceSongProjection,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MatchKind {
    NumericId,
    ExactTitle,
    ExactAlias,
    TitlePrefix,
    TitleContains,
    AliasPrefix,
    AliasContains,
    PinyinExact,
    PinyinPrefix,
    PinyinContains,
    KeywordContains,
    FilterOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SongIdFilter {
    /// Match the value in any source namespace.
    AnySource(SongIdValue),
    /// Match one source-aware stable identifier.
    Exact(SourceSongId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InclusiveRange<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

impl<T> InclusiveRange<T> {
    pub const fn new(min: Option<T>, max: Option<T>) -> Self {
        Self { min, max }
    }

    pub const fn unbounded() -> Self {
        Self {
            min: None,
            max: None,
        }
    }
}

impl<T> Default for InclusiveRange<T> {
    fn default() -> Self {
        Self::unbounded()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortKey {
    #[default]
    Relevance,
    Title,
    Id,
    Bpm,
    Constant,
    FitDifference,
    FitDelta,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CatalogSort {
    pub key: SortKey,
    pub direction: SortDirection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FitLabel {
    Inflated,
    Deflated,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NewSongSource {
    #[default]
    Any,
    China,
    Japan,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatalogQuery {
    pub query: Option<String>,
    pub id: Option<SongIdFilter>,
    pub id_range: InclusiveRange<u32>,
    pub level: Option<String>,
    pub genre: Option<String>,
    pub version: Option<String>,
    pub constant: InclusiveRange<ChartConstant>,
    pub fit_diff: InclusiveRange<Decimal>,
    pub fit_diff_max_exclusive: bool,
    pub fit_delta: InclusiveRange<Decimal>,
    pub fit_label: Option<FitLabel>,
    pub difficulties: BTreeSet<Difficulty>,
    pub generations: BTreeSet<ChartGeneration>,
    pub bpm: InclusiveRange<u32>,
    pub artist: Option<String>,
    pub charter: Option<String>,
    pub region_has: BTreeSet<Region>,
    pub region_missing: BTreeSet<Region>,
    pub is_new: Option<bool>,
    pub is_new_source: NewSongSource,
    pub is_locked: Option<bool>,
    pub required_tag_ids: BTreeSet<u32>,
    pub excluded_tag_ids: BTreeSet<u32>,
    pub released_after: Option<Date>,
    pub released_before: Option<Date>,
    pub sort: CatalogSort,
    pub limit: Option<usize>,
}

impl CatalogQuery {
    pub fn text(query: impl Into<String>, limit: usize) -> Self {
        Self {
            query: Some(query.into()),
            limit: Some(limit),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchedChart<'a> {
    pub chart: &'a Chart,
    pub metadata: &'a ChartQueryMetadata,
    pub constant_display: Option<String>,
    pub source_matches: Vec<SourceChartMatch<'a>>,
    pub(crate) chart_index: usize,
}

impl<'a> MatchedChart<'a> {
    pub(crate) fn new(
        chart: &'a Chart,
        metadata: &'a ChartQueryMetadata,
        chart_index: usize,
        source_matches: Vec<SourceChartMatch<'a>>,
    ) -> Self {
        Self {
            chart,
            metadata,
            constant_display: chart.constant.map(format_constant),
            source_matches,
            chart_index,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceChartMatch<'a> {
    pub song: &'a SourceSongProjection,
    pub chart: &'a SourceChartProjection,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit<'a> {
    pub music: &'a Music,
    pub metadata: &'a SongQueryMetadata,
    pub matched_by: MatchKind,
    pub matched_value: Option<String>,
    pub matched_charts: Vec<MatchedChart<'a>>,
    pub(crate) song_index: usize,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum QueryError {
    #[error("{field} minimum cannot be greater than maximum")]
    InvalidRange { field: &'static str },

    #[error("limit must be greater than zero")]
    InvalidLimit,

    #[error("unknown catalog tag: {0}")]
    UnknownTag(String),
}

fn format_constant(constant: ChartConstant) -> String {
    let mut value = constant.value().normalize().to_string();
    if !value.contains('.') {
        value.push_str(".0");
    }
    value
}
