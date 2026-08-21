use crate::{CatalogSnapshot, ChartQueryMetadata, SourceChartProjection, SourceSongProjection};

use super::{
    CatalogQuery,
    filters::{contains_filter, fit_label_matches, in_range, name_matches, normalize_level},
};

pub(super) fn source_chart_matches(
    snapshot: &CatalogSnapshot,
    song: &SourceSongProjection,
    chart: &SourceChartProjection,
    metadata: &ChartQueryMetadata,
    query: &CatalogQuery,
) -> bool {
    if !query.difficulties.is_empty() && !query.difficulties.contains(&chart.difficulty) {
        return false;
    }
    if !query.generations.is_empty() && !query.generations.contains(&chart.generation) {
        return false;
    }
    if query
        .level
        .as_ref()
        .is_some_and(|level| normalize_level(&chart.level) != normalize_level(level))
    {
        return false;
    }
    if query.constant.min.is_some() || query.constant.max.is_some() {
        let Some(value) = chart.constant else {
            return false;
        };
        let Ok(value) = maimai_core::ChartConstant::from_decimal_str(&value.to_string()) else {
            return false;
        };
        if !in_range(value, query.constant.min, query.constant.max, false) {
            return false;
        }
    }
    if !name_matches(
        snapshot,
        &chart.note_designer,
        query.charter.as_deref(),
        &snapshot.metadata().charter_aliases,
    ) {
        return false;
    }
    if query
        .version
        .as_ref()
        .is_some_and(|expected| !version_matches(snapshot, song, chart, expected))
    {
        return false;
    }
    let fit = chart.fit_stats.as_ref().and_then(|stats| stats.fit_diff);
    if (query.fit_diff.min.is_some() || query.fit_diff.max.is_some())
        && fit.is_none_or(|value| {
            !in_range(
                value,
                query.fit_diff.min,
                query.fit_diff.max,
                query.fit_diff_max_exclusive,
            )
        })
    {
        return false;
    }
    let delta = chart
        .constant
        .zip(fit)
        .map(|(constant, fit)| constant - fit);
    if (query.fit_delta.min.is_some() || query.fit_delta.max.is_some())
        && delta
            .is_none_or(|value| !in_range(value, query.fit_delta.min, query.fit_delta.max, false))
    {
        return false;
    }
    if query
        .fit_label
        .is_some_and(|label| !fit_label_matches(label, delta))
    {
        return false;
    }
    if !query
        .region_has
        .iter()
        .all(|region| chart.regions.has(*region))
        || query
            .region_missing
            .iter()
            .any(|region| chart.regions.has(*region))
    {
        return false;
    }
    if !query
        .required_tag_ids
        .iter()
        .all(|tag| metadata.tag_ids.contains(tag))
        || query
            .excluded_tag_ids
            .iter()
            .any(|tag| metadata.tag_ids.contains(tag))
    {
        return false;
    }
    if query
        .released_after
        .is_some_and(|bound| chart.release_date.is_none_or(|date| date < bound))
        || query
            .released_before
            .is_some_and(|bound| chart.release_date.is_none_or(|date| date > bound))
    {
        return false;
    }
    true
}

fn version_matches(
    snapshot: &CatalogSnapshot,
    song: &SourceSongProjection,
    chart: &SourceChartProjection,
    expected: &str,
) -> bool {
    expected
        .split(['/', ',', '，', '、', '|'])
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .any(|term| {
            if let Some(constraint) = lxns_version_constraint(term) {
                return song
                    .source_fields
                    .numeric_version
                    .is_some_and(|version| constraint.matches(version));
            }
            contains_filter(snapshot, &song.version, Some(term))
                || contains_filter(snapshot, &chart.version, Some(term))
        })
}

enum LxnsVersionConstraint {
    Exact(u32),
    Year(u32),
}

impl LxnsVersionConstraint {
    const fn matches(&self, value: u32) -> bool {
        match self {
            Self::Exact(expected) => value == *expected,
            Self::Year(year) => value / 1_000 == *year,
        }
    }
}

fn lxns_version_constraint(value: &str) -> Option<LxnsVersionConstraint> {
    let mut value = value.trim().to_ascii_lowercase().replace(' ', "");
    for suffix in ["版本", "版", "年"] {
        if let Some(stripped) = value.strip_suffix(suffix) {
            value = stripped.to_owned();
            break;
        }
    }
    for prefix in ["舞萌dx", "舞萌", "maimaidx", "maimai", "dx"] {
        if let Some(stripped) = value.strip_prefix(prefix)
            && stripped
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
        {
            value = stripped.to_owned();
            break;
        }
    }
    if value.len() == 5 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse().ok().map(LxnsVersionConstraint::Exact);
    }
    let year = if value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        value.parse().ok()
    } else if value.len() == 4
        && value.starts_with("20")
        && value.bytes().all(|byte| byte.is_ascii_digit())
    {
        value[2..].parse().ok()
    } else {
        None
    };
    if let Some(year) = year {
        return Some(LxnsVersionConstraint::Year(year));
    }
    let normalized = value.replace('_', "-");
    let (year, sub) = normalized.split_once('-')?;
    let year = year
        .strip_prefix("20")
        .unwrap_or(year)
        .parse::<u32>()
        .ok()?;
    let sub = sub.parse::<u32>().ok()?;
    (sub <= 999).then_some(LxnsVersionConstraint::Exact(year * 1_000 + sub))
}
