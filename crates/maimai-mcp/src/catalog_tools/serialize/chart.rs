use std::collections::BTreeSet;

use maimai_catalog::{
    CatalogSnapshot, MatchedChart, SearchHit, SourceChartProjection, SourceSongProjection,
};
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};

use super::{
    chart_type, decimal_value, difficulty, difficulty_index, regions, source_id_value,
    source_priority,
};

pub(super) fn matched_source_charts(snapshot: &CatalogSnapshot, hit: &SearchHit<'_>) -> Vec<Value> {
    let mut matches = hit
        .matched_charts
        .iter()
        .flat_map(|matched| {
            matched
                .source_matches
                .iter()
                .map(move |source| (matched, source.song, source.chart))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(_, source, chart)| {
        (
            source_priority(source.source),
            chart.generation,
            chart.difficulty,
        )
    });
    matches
        .into_iter()
        .map(|(matched, source, chart)| source_chart_value(snapshot, source, chart, matched))
        .collect()
}

fn source_chart_value(
    snapshot: &CatalogSnapshot,
    source: &SourceSongProjection,
    chart: &SourceChartProjection,
    matched: &MatchedChart<'_>,
) -> Value {
    let mut output = base_chart(
        chart.generation,
        chart.difficulty,
        &chart.level,
        chart.constant,
        &chart.note_designer,
    );
    if let Some(raw_difficulty) = &chart.raw_difficulty {
        output.insert("difficulty".to_owned(), json!(raw_difficulty));
    }
    output.insert("source".to_owned(), json!(source.source.key()));
    output.insert("source_name".to_owned(), json!(source.source.source_name()));
    output.insert("source_id".to_owned(), json!(source_id_value(&source.id)));
    output.insert("music_id".to_owned(), json!(chart.music_id));
    output.insert("chart_id".to_owned(), json!(chart.chart_id));
    output.insert("internal_id".to_owned(), json!(chart.internal_id));
    output.insert("version".to_owned(), json!(chart.version));
    if let Some(notes) = chart.notes {
        output.insert(
            "notes".to_owned(),
            json!({
                "total": chart.note_total.map_or(notes.total(), u64::from),
                "tap": notes.tap,
                "hold": notes.hold,
                "slide": notes.slide,
                "touch": notes.touch,
                "break": notes.break_notes,
            }),
        );
    }
    output.insert("regions".to_owned(), regions(chart.regions));
    output.insert(
        "release_date".to_owned(),
        json!(chart.release_date.map(|value| value.to_string())),
    );
    output.insert("is_buddy".to_owned(), json!(chart.is_buddy));
    output.insert("kanji".to_owned(), json!(chart.kanji));
    output.insert("description".to_owned(), json!(chart.description));
    if chart.is_special {
        output.insert("is_special".to_owned(), json!(true));
    }
    if !chart.region_overrides.is_empty() {
        output.insert(
            "region_overrides".to_owned(),
            Value::Object(
                chart
                    .region_overrides
                    .iter()
                    .map(|(region, value)| {
                        let mut fields = Map::new();
                        if let Some(level) = &value.level {
                            fields.insert("level".to_owned(), json!(level));
                        }
                        if let Some(constant) = value.constant {
                            fields.insert("levelValue".to_owned(), decimal_value(constant));
                        }
                        if let Some(version) = &value.version {
                            fields.insert("version".to_owned(), json!(version));
                        }
                        (region.clone(), Value::Object(fields))
                    })
                    .collect(),
            ),
        );
    }
    if !chart.multiver_constants.is_empty() {
        output.insert(
            "multiver_internal_level_value".to_owned(),
            decimal_map(&chart.multiver_constants),
        );
    }
    let fit_diff = chart.fit_stats.as_ref().and_then(|stats| stats.fit_diff);
    insert_fit(&mut output, chart.constant, fit_diff);
    output.insert("fit_source_id".to_owned(), json!(chart.fit_source_id));
    if let Some(stats) = &chart.fit_stats {
        let mut fields = Map::new();
        if let Some(value) = stats.count {
            fields.insert("cnt".to_owned(), decimal_value(value));
        }
        if let Some(value) = &stats.diff {
            fields.insert("diff".to_owned(), json!(value));
        }
        if let Some(value) = stats.average {
            fields.insert("avg".to_owned(), decimal_value(value));
        }
        if let Some(value) = stats.average_dx {
            fields.insert("avg_dx".to_owned(), decimal_value(value));
        }
        if let Some(value) = stats.standard_deviation {
            fields.insert("std_dev".to_owned(), decimal_value(value));
        }
        if !stats.distribution.is_empty() {
            fields.insert(
                "dist".to_owned(),
                Value::Array(
                    stats
                        .distribution
                        .iter()
                        .copied()
                        .map(decimal_value)
                        .collect(),
                ),
            );
        }
        if !stats.full_combo_distribution.is_empty() {
            fields.insert(
                "fc_dist".to_owned(),
                Value::Array(
                    stats
                        .full_combo_distribution
                        .iter()
                        .copied()
                        .map(decimal_value)
                        .collect(),
                ),
            );
        }
        output.insert("fit_stats".to_owned(), Value::Object(fields));
    }
    insert_tags(snapshot, &mut output, matched);
    Value::Object(output)
}

fn base_chart(
    generation: maimai_core::ChartGeneration,
    chart_difficulty: maimai_core::Difficulty,
    level: &str,
    constant: Option<Decimal>,
    charter: &str,
) -> Map<String, Value> {
    let mut output = Map::new();
    output.insert("chart_type".to_owned(), json!(chart_type(generation)));
    output.insert(
        "difficulty_index".to_owned(),
        json!(difficulty_index(chart_difficulty)),
    );
    output.insert("difficulty".to_owned(), json!(difficulty(chart_difficulty)));
    output.insert("level".to_owned(), json!(level));
    output.insert("ds".to_owned(), constant.map_or(Value::Null, decimal_value));
    output.insert("charter".to_owned(), json!(charter));
    output
}

fn insert_fit(output: &mut Map<String, Value>, constant: Option<Decimal>, fit: Option<Decimal>) {
    output.insert(
        "fit_diff".to_owned(),
        fit.map_or(Value::Null, decimal_value),
    );
    let delta = constant.zip(fit).map(|(constant, fit)| constant - fit);
    output.insert(
        "fit_delta".to_owned(),
        delta.map_or(Value::Null, decimal_value),
    );
    output.insert(
        "fit_label".to_owned(),
        json!(delta.and_then(|value| {
            if value > Decimal::ZERO {
                Some("虚高")
            } else if value < Decimal::ZERO {
                Some("虚低")
            } else {
                None
            }
        })),
    );
}

fn insert_tags(
    snapshot: &CatalogSnapshot,
    output: &mut Map<String, Value>,
    matched: &MatchedChart<'_>,
) {
    output.insert(
        "tags".to_owned(),
        Value::Array(
            matched
                .metadata
                .tag_ids
                .iter()
                .map(|id| json!({"id": id, "name": snapshot.tag_label(*id)}))
                .collect(),
        ),
    );
}

fn decimal_map(values: &std::collections::BTreeMap<String, Decimal>) -> Value {
    Value::Object(
        values
            .iter()
            .map(|(key, value)| (key.clone(), decimal_value(*value)))
            .collect(),
    )
}

pub(super) fn source_chart_types(projections: &[&SourceSongProjection]) -> Vec<&'static str> {
    projections
        .iter()
        .flat_map(|source| {
            source
                .charts
                .iter()
                .map(|chart| chart_type(chart.generation))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
