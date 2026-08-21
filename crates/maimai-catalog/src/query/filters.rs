use std::collections::{BTreeSet, HashMap};

use maimai_core::{Chart, Music, SongIdValue};
use rust_decimal::Decimal;

use crate::{CatalogSnapshot, ChartQueryMetadata, SongQueryMetadata};

use super::{CatalogQuery, FitLabel, NewSongSource, QueryError, SongIdFilter};

pub(super) fn validate(query: &CatalogQuery) -> Result<(), QueryError> {
    if query.limit == Some(0) {
        return Err(QueryError::InvalidLimit);
    }
    validate_range(query.id_range.min, query.id_range.max, "id")?;
    validate_range(query.constant.min, query.constant.max, "constant")?;
    validate_range(query.fit_diff.min, query.fit_diff.max, "fit_diff")?;
    validate_range(query.fit_delta.min, query.fit_delta.max, "fit_delta")?;
    validate_range(query.bpm.min, query.bpm.max, "bpm")
}

pub(super) fn music_matches(
    snapshot: &CatalogSnapshot,
    music: &Music,
    metadata: &SongQueryMetadata,
    query: &CatalogQuery,
) -> bool {
    if query
        .id
        .as_ref()
        .is_some_and(|filter| !id_matches(music, metadata, filter))
    {
        return false;
    }
    if query.id_range.min.is_some() || query.id_range.max.is_some() {
        let matches = metadata
            .canonical_numeric_ids
            .iter()
            .copied()
            .any(|value| in_range(value, query.id_range.min, query.id_range.max, false));
        if !matches {
            return false;
        }
    }
    if !in_range(music.bpm, query.bpm.min, query.bpm.max, false) {
        return false;
    }
    if !genre_matches(snapshot, music, metadata, query.genre.as_deref())
        || !artist_matches(snapshot, music, metadata, query.artist.as_deref())
    {
        return false;
    }
    if !query
        .region_has
        .iter()
        .all(|region| metadata.regions.has(*region))
        || query
            .region_missing
            .iter()
            .any(|region| metadata.regions.has(*region))
    {
        return false;
    }
    if let Some(expected) = query.is_new {
        let actual = match query.is_new_source {
            NewSongSource::Any => metadata.is_new_cn || metadata.is_new_jp,
            NewSongSource::China => metadata.is_new_cn,
            NewSongSource::Japan => metadata.is_new_jp,
        };
        if actual != expected {
            return false;
        }
    }
    if query
        .is_locked
        .is_some_and(|expected| metadata.is_locked.unwrap_or(false) != expected)
    {
        return false;
    }
    true
}

pub(super) fn chart_matches(
    snapshot: &CatalogSnapshot,
    music: &Music,
    chart: &Chart,
    song_metadata: &SongQueryMetadata,
    metadata: &ChartQueryMetadata,
    query: &CatalogQuery,
) -> bool {
    if !query.difficulties.is_empty() && !query.difficulties.contains(&chart.key.difficulty()) {
        return false;
    }
    if !query.generations.is_empty() && !query.generations.contains(&chart.key.generation()) {
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
        let Some(constant) = chart.constant else {
            return false;
        };
        if !in_range(constant, query.constant.min, query.constant.max, false) {
            return false;
        }
    }
    if !charter_matches(snapshot, chart, song_metadata, query.charter.as_deref()) {
        return false;
    }
    if query.version.as_ref().is_some_and(|expected| {
        !contains_filter(snapshot, &music.version, Some(expected))
            && !metadata
                .version
                .as_deref()
                .is_some_and(|version| contains_filter(snapshot, version, Some(expected)))
            && !song_metadata.source_projections.iter().any(|source| {
                contains_filter(snapshot, &source.version, Some(expected))
                    || source
                        .source_fields
                        .numeric_version
                        .is_some_and(|version| version.to_string().starts_with(expected))
                    || source.charts.iter().any(|source_chart| {
                        source_chart.generation == chart.key.generation()
                            && source_chart.difficulty == chart.key.difficulty()
                            && contains_filter(snapshot, &source_chart.version, Some(expected))
                    })
            })
    }) {
        return false;
    }
    if query.fit_diff.min.is_some() || query.fit_diff.max.is_some() {
        let Some(fit) = metadata.fit_diff else {
            return false;
        };
        if !in_range(
            fit,
            query.fit_diff.min,
            query.fit_diff.max,
            query.fit_diff_max_exclusive,
        ) {
            return false;
        }
    }
    let fit_delta = chart
        .constant
        .zip(metadata.fit_diff)
        .map(|(constant, fit)| constant.value() - fit);
    if query.fit_delta.min.is_some() || query.fit_delta.max.is_some() {
        let Some(delta) = fit_delta else {
            return false;
        };
        if !in_range(delta, query.fit_delta.min, query.fit_delta.max, false) {
            return false;
        }
    }
    if query
        .fit_label
        .is_some_and(|label| !fit_label_matches(label, fit_delta))
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
    if query.released_after.as_ref().is_some_and(|bound| {
        metadata
            .release_date
            .as_ref()
            .is_none_or(|date| date < bound)
    }) || query.released_before.as_ref().is_some_and(|bound| {
        metadata
            .release_date
            .as_ref()
            .is_none_or(|date| date > bound)
    }) {
        return false;
    }
    true
}

fn id_matches(music: &Music, metadata: &SongQueryMetadata, filter: &SongIdFilter) -> bool {
    match filter {
        SongIdFilter::AnySource(SongIdValue::Numeric(value)) => {
            metadata.canonical_numeric_ids.contains(value)
        }
        SongIdFilter::AnySource(value) => music
            .source_ids
            .iter()
            .any(|source_id| source_id.value() == value),
        SongIdFilter::Exact(expected) => music
            .source_ids
            .iter()
            .any(|source_id| source_id == expected),
    }
}

pub(super) fn contains_filter(
    snapshot: &CatalogSnapshot,
    actual: &str,
    expected: Option<&str>,
) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let needle = snapshot.normalizer().normalize(expected);
    needle.is_empty() || snapshot.normalizer().normalize(actual).contains(&needle)
}

fn genre_matches(
    snapshot: &CatalogSnapshot,
    music: &Music,
    metadata: &SongQueryMetadata,
    expected: Option<&str>,
) -> bool {
    std::iter::once(music.genre.as_str())
        .chain(
            metadata
                .source_projections
                .iter()
                .map(|source| source.genre.as_str()),
        )
        .any(|actual| contains_filter(snapshot, actual, expected))
}

fn artist_matches(
    snapshot: &CatalogSnapshot,
    music: &Music,
    metadata: &SongQueryMetadata,
    expected: Option<&str>,
) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    std::iter::once(music.artist.as_str())
        .chain(
            metadata
                .source_projections
                .iter()
                .map(|source| source.artist.as_str()),
        )
        .any(|actual| {
            name_matches(
                snapshot,
                actual,
                Some(expected),
                &snapshot.metadata().artist_aliases,
            )
        })
}

fn charter_matches(
    snapshot: &CatalogSnapshot,
    chart: &Chart,
    metadata: &SongQueryMetadata,
    expected: Option<&str>,
) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let candidates = name_candidates(snapshot, expected, &snapshot.metadata().charter_aliases);
    std::iter::once(chart.note_designer.as_str())
        .chain(metadata.source_projections.iter().flat_map(|source| {
            source
                .charts
                .iter()
                .filter(move |source_chart| {
                    source_chart.generation == chart.key.generation()
                        && source_chart.difficulty == chart.key.difficulty()
                })
                .map(|source_chart| source_chart.note_designer.as_str())
        }))
        .any(|actual| name_contains(snapshot, actual, &candidates))
}

fn name_candidates(
    snapshot: &CatalogSnapshot,
    expected: &str,
    aliases: &HashMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let needle = snapshot.normalizer().normalize(expected);
    let mut result = BTreeSet::from([needle.clone()]);
    for (name, group) in aliases {
        if name.contains(&needle) || needle.contains(name) {
            result.extend(group.iter().cloned());
        }
    }
    result
}

fn name_contains(snapshot: &CatalogSnapshot, actual: &str, candidates: &BTreeSet<String>) -> bool {
    let actual = snapshot.normalizer().normalize(actual);
    candidates
        .iter()
        .any(|candidate| actual.contains(candidate))
}

pub(super) fn name_matches(
    snapshot: &CatalogSnapshot,
    actual: &str,
    expected: Option<&str>,
    aliases: &HashMap<String, BTreeSet<String>>,
) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let candidates = name_candidates(snapshot, expected, aliases);
    name_contains(snapshot, actual, &candidates)
}

pub(super) fn normalize_level(value: &str) -> String {
    value.trim().trim_end_matches('?').to_ascii_lowercase()
}

pub(super) fn fit_label_matches(label: FitLabel, delta: Option<Decimal>) -> bool {
    match (label, delta) {
        (FitLabel::Inflated, Some(delta)) => delta > Decimal::ZERO,
        (FitLabel::Deflated, Some(delta)) => delta < Decimal::ZERO,
        (_, None) => false,
    }
}

fn validate_range<T: Copy + PartialOrd>(
    min: Option<T>,
    max: Option<T>,
    field: &'static str,
) -> Result<(), QueryError> {
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        Err(QueryError::InvalidRange { field })
    } else {
        Ok(())
    }
}

pub(super) fn in_range<T: Copy + PartialOrd>(
    value: T,
    min: Option<T>,
    max: Option<T>,
    max_exclusive: bool,
) -> bool {
    if min.is_some_and(|min| value < min) {
        return false;
    }
    !max.is_some_and(|max| {
        if max_exclusive {
            value >= max
        } else {
            value > max
        }
    })
}
