use std::cmp::Ordering;

use maimai_core::ChartConstant;

use crate::{CatalogSnapshot, SourceKind};

use super::{CatalogSort, SearchHit, SortDirection, SortKey};

pub(super) fn sort_hits(snapshot: &CatalogSnapshot, hits: &mut [SearchHit<'_>], sort: CatalogSort) {
    hits.sort_by(|left, right| {
        let primary = match sort.key {
            SortKey::Relevance => {
                apply_direction(left.matched_by.cmp(&right.matched_by), sort.direction)
            }
            SortKey::Title => apply_direction(
                normalized_title(snapshot, left).cmp(normalized_title(snapshot, right)),
                sort.direction,
            ),
            SortKey::Id => apply_direction(sort_id(left).cmp(&sort_id(right)), sort.direction),
            SortKey::Bpm => apply_direction(left.music.bpm.cmp(&right.music.bpm), sort.direction),
            SortKey::Constant => compare_optional_constant(
                sort_constant(left, sort.direction),
                sort_constant(right, sort.direction),
                sort.direction,
            ),
            SortKey::FitDifference => compare_optional_decimal(
                sort_decimal(left, false, sort.direction),
                sort_decimal(right, false, sort.direction),
                sort.direction,
            ),
            SortKey::FitDelta => compare_optional_decimal(
                sort_decimal(left, true, sort.direction),
                sort_decimal(right, true, sort.direction),
                sort.direction,
            ),
        };
        primary
            .then_with(|| normalized_title(snapshot, left).cmp(normalized_title(snapshot, right)))
            .then_with(|| left.music.primary_id.cmp(&right.music.primary_id))
    });
}

fn sort_id(hit: &SearchHit<'_>) -> Option<u32> {
    let mut sources = hit.metadata.source_projections.iter().collect::<Vec<_>>();
    sources.sort_by_key(|source| match source.source {
        SourceKind::China => 0,
        SourceKind::Official => 1,
        SourceKind::Japan => 2,
        SourceKind::DivingFish => 3,
    });
    for source in sources {
        let maimai_core::SongIdValue::Numeric(value) = source.id.value() else {
            continue;
        };
        if source.source == SourceKind::DivingFish
            && (10_000..100_000).contains(value)
            && source
                .charts
                .iter()
                .any(|chart| chart.generation == maimai_core::ChartGeneration::Deluxe)
        {
            return Some(*value - 10_000);
        }
        return Some(*value);
    }
    hit.metadata.canonical_numeric_ids.iter().next().copied()
}

fn sort_decimal(
    hit: &SearchHit<'_>,
    delta: bool,
    direction: SortDirection,
) -> Option<rust_decimal::Decimal> {
    let values = hit.matched_charts.iter().filter_map(|matched| {
        let fit = matched.metadata.fit_diff?;
        if delta {
            matched
                .chart
                .constant
                .map(|constant| constant.value() - fit)
        } else {
            Some(fit)
        }
    });
    match direction {
        SortDirection::Ascending => values.min(),
        SortDirection::Descending => values.max(),
    }
}

fn normalized_title<'a>(snapshot: &'a CatalogSnapshot, hit: &SearchHit<'_>) -> &'a str {
    snapshot
        .search_index()
        .entry(hit.song_index)
        .map_or("", |entry| entry.title.as_str())
}

fn sort_constant(hit: &SearchHit<'_>, direction: SortDirection) -> Option<ChartConstant> {
    let values = hit
        .matched_charts
        .iter()
        .filter_map(|matched| matched.chart.constant);
    match direction {
        SortDirection::Ascending => values.min(),
        SortDirection::Descending => values.max(),
    }
}

fn compare_optional_constant(
    left: Option<ChartConstant>,
    right: Option<ChartConstant>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => apply_direction(left.cmp(&right), direction),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_decimal(
    left: Option<rust_decimal::Decimal>,
    right: Option<rust_decimal::Decimal>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => apply_direction(left.cmp(&right), direction),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn apply_direction(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}
