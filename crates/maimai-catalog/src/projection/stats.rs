use rust_decimal::Decimal;
use serde_json::Number;

use crate::{
    CatalogError,
    metadata::ChartFitStats,
    raw::{ChartStat, ChartStatsDocument},
};

pub(super) fn fit_stats(
    raw: &ChartStat,
    source: &'static str,
    song_id: &str,
) -> Result<ChartFitStats, CatalogError> {
    let parse = |value: &Option<Number>, field| {
        value
            .as_ref()
            .map(|value| number_decimal(value, source, song_id, field))
            .transpose()
    };
    Ok(ChartFitStats {
        count: parse(&raw.cnt, "cnt")?,
        diff: raw.diff.clone(),
        fit_diff: parse(&raw.fit_diff, "fit_diff")?,
        average: parse(&raw.avg, "avg")?,
        average_dx: parse(&raw.avg_dx, "avg_dx")?,
        standard_deviation: parse(&raw.std_dev, "std_dev")?,
        distribution: decimal_list(&raw.dist, source, song_id, "dist")?,
        full_combo_distribution: decimal_list(&raw.fc_dist, source, song_id, "fc_dist")?,
    })
}

pub(super) fn number_decimal(
    value: &Number,
    source_name: &'static str,
    song_id: &str,
    field: &'static str,
) -> Result<Decimal, CatalogError> {
    value
        .to_string()
        .parse::<Decimal>()
        .map_err(|_| CatalogError::InvalidNumber {
            source_name,
            song_id: song_id.to_owned(),
            field,
            value: value.to_string(),
        })
}

pub(super) fn stat_for<'a>(
    stats: &'a ChartStatsDocument,
    id: u32,
    index: usize,
    level: &str,
) -> Option<&'a ChartStat> {
    stats
        .charts
        .get(&id.to_string())
        .and_then(|values| values.get(index))
        .filter(|stat| stat.fit_diff.is_some())
        .filter(|stat| {
            stat.diff
                .as_ref()
                .is_none_or(|value| normalize_level(value) == normalize_level(level))
        })
}

fn normalize_level(value: &str) -> String {
    value.trim().trim_end_matches('?').to_ascii_lowercase()
}

fn decimal_list(
    values: &[Number],
    source: &'static str,
    song_id: &str,
    field: &'static str,
) -> Result<Vec<Decimal>, CatalogError> {
    values
        .iter()
        .map(|value| number_decimal(value, source, song_id, field))
        .collect()
}
