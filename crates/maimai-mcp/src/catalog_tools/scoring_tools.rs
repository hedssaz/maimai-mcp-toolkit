use std::collections::HashSet;

use maimai_catalog::{CatalogSnapshot, MatchedChart, SearchHit, SourceChartMatch};
use maimai_core::NoteCounts;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{
    convert::catalog_query,
    dto::SearchArgs,
    error::CatalogToolError,
    format::FormatOptions,
    serialize::{chart_type, decimal_value, difficulty, song_value, source_priority},
    static_format,
};

const LOOKUP_KEYS: [&str; 20] = [
    "query",
    "song_id",
    "title",
    "level",
    "genre",
    "version",
    "ds",
    "ds_min",
    "ds_max",
    "fit_diff",
    "fit_diff_min",
    "fit_diff_max",
    "fit_delta",
    "fit_delta_min",
    "fit_delta_max",
    "fit_label",
    "region_has",
    "region_missing",
    "difficulty",
    "song_type",
];

#[derive(Deserialize)]
struct CatalogScoringArgs {
    format: Option<String>,
    include_raw: Option<bool>,
    debug: Option<bool>,
    #[serde(flatten)]
    arguments: Map<String, Value>,
}

pub(super) fn execute(
    snapshot: &CatalogSnapshot,
    tool: &str,
    arguments: Map<String, Value>,
) -> Result<(Value, FormatOptions<'static>), CatalogToolError> {
    let arguments: CatalogScoringArgs =
        serde_json::from_value(Value::Object(arguments)).map_err(|error| {
            CatalogToolError::input(format!("Invalid arguments for tool {tool}: {error}"))
        })?;
    let options = static_format(&arguments.format, arguments.include_raw, arguments.debug)?;
    let value = match tool {
        "score_counts" => crate::scoring::score_counts_value(arguments.arguments)
            .map_err(|error| CatalogToolError::input(error.to_string()))?,
        "find_score_combinations" => find_combinations(snapshot, arguments.arguments)?,
        _ => {
            return Err(CatalogToolError::input(format!(
                "unknown scoring tool: {tool}"
            )));
        }
    };
    Ok((value, options))
}

fn find_combinations(
    snapshot: &CatalogSnapshot,
    mut arguments: Map<String, Value>,
) -> Result<Value, CatalogToolError> {
    if arguments
        .get("note_totals")
        .is_some_and(|value| !value.is_null())
    {
        remove_lookup_fields(&mut arguments);
        return crate::scoring::find_combinations_value(arguments)
            .map_err(|error| CatalogToolError::input(error.to_string()));
    }

    let resolution = resolve_chart(snapshot, &arguments)?;
    if resolution.get("resolved") != Some(&Value::Bool(true)) {
        return Ok(resolution);
    }
    let note_totals = resolution
        .get("note_totals")
        .cloned()
        .ok_or_else(|| CatalogToolError::input("resolved chart is missing note totals"))?;
    remove_lookup_fields(&mut arguments);
    arguments.insert("note_totals".to_owned(), note_totals);
    let mut result = crate::scoring::find_combinations_value(arguments)
        .map_err(|error| CatalogToolError::input(error.to_string()))?;
    let output = result
        .as_object_mut()
        .ok_or_else(|| CatalogToolError::input("scoring result must be an object"))?;
    output.insert("calculated".to_owned(), Value::Bool(true));
    output.insert("lookup".to_owned(), resolution);
    Ok(result)
}

fn resolve_chart(
    snapshot: &CatalogSnapshot,
    arguments: &Map<String, Value>,
) -> Result<Value, CatalogToolError> {
    let query = direct_query(arguments)?
        .ok_or_else(|| CatalogToolError::input("note_totals or query/song_id/title is required"))?;
    let mut search_fields = Map::new();
    search_fields.insert("query".to_owned(), Value::String(query));
    for key in LOOKUP_KEYS.iter().skip(3).chain([&"artist", &"charter"]) {
        if let Some(value) = arguments.get(*key).filter(|value| !value.is_null()) {
            search_fields.insert((*key).to_owned(), value.clone());
        }
    }
    let criteria = Value::Object(search_fields.clone());
    let search: SearchArgs = serde_json::from_value(Value::Object(search_fields))
        .map_err(|error| CatalogToolError::input(format!("Invalid chart lookup: {error}")))?;
    let mut query = catalog_query(snapshot, &search)?;
    query.limit = None;
    let hits = snapshot.query(&query)?;

    if hits.is_empty() {
        return Ok(json!({
            "resolved": false,
            "reason": "no_song_match",
            "calculated": false,
            "criteria": criteria,
            "songs": [],
        }));
    }
    if hits.len() > 1 {
        let mut result = json!({
            "resolved": false,
            "reason": "multiple_song_matches",
            "calculated": false,
            "criteria": criteria,
            "total_matches": hits.len(),
            "truncated": hits.len() > 20,
            "songs": hits.iter().take(20).map(|hit| song_summary(snapshot, hit)).collect::<Vec<_>>(),
        });
        omit_false_truncated(&mut result);
        return Ok(result);
    }

    let hit = &hits[0];
    let charts = deduplicated_charts(&hit.matched_charts);
    if charts.len() != 1 {
        let mut result = json!({
            "resolved": false,
            "reason": if charts.is_empty() { "no_chart_match" } else { "multiple_chart_matches" },
            "calculated": false,
            "criteria": criteria,
            "song": song_summary(snapshot, hit),
            "charts": charts.iter().take(20).map(|chart| chart_summary(chart)).collect::<Vec<_>>(),
            "total_charts": charts.len(),
            "truncated": charts.len() > 20,
        });
        omit_false_truncated(&mut result);
        return Ok(result);
    }

    let chart = charts[0];
    let notes = chart.chart.notes;
    let song = song_value(snapshot, hit);
    Ok(json!({
        "resolved": true,
        "calculated": false,
        "criteria": criteria,
        "song": {
            "id": song.get("id"),
            "title": song.get("title"),
            "artist": song.get("artist"),
            "source": song.get("source"),
            "source_ids": song.get("source_ids"),
        },
        "chart": chart_summary(chart),
        "note_totals": scoring_note_totals(notes),
        "original_note_counts": original_note_counts(notes),
        "touch_scored_as_tap": notes.touch > 0,
    }))
}

fn omit_false_truncated(value: &mut Value) {
    let should_omit = value.get("truncated").and_then(Value::as_bool) == Some(false);
    if let Some(output) = value.as_object_mut().filter(|_| should_omit) {
        output.remove("truncated");
    }
}

fn direct_query(arguments: &Map<String, Value>) -> Result<Option<String>, CatalogToolError> {
    for key in ["query", "song_id", "title"] {
        let Some(value) = arguments.get(key).filter(|value| !value.is_null()) else {
            continue;
        };
        let text = match value {
            Value::String(value) => value.trim().to_owned(),
            Value::Number(value) => value.to_string(),
            _ => {
                return Err(CatalogToolError::input(format!(
                    "{key} must be a string or number"
                )));
            }
        };
        if !text.is_empty() {
            return Ok(Some(text));
        }
    }
    Ok(None)
}

fn remove_lookup_fields(arguments: &mut Map<String, Value>) {
    for key in LOOKUP_KEYS.into_iter().chain(["artist", "charter"]) {
        arguments.remove(key);
    }
}

fn deduplicated_charts<'a>(charts: &'a [MatchedChart<'a>]) -> Vec<&'a MatchedChart<'a>> {
    let mut seen = HashSet::new();
    charts
        .iter()
        .filter(|chart| {
            seen.insert((
                chart.chart.key.generation(),
                chart.chart.key.difficulty(),
                chart.chart.level.as_str(),
                chart.chart.constant,
            ))
        })
        .collect()
}

fn song_summary(snapshot: &CatalogSnapshot, hit: &SearchHit<'_>) -> Value {
    let song = song_value(snapshot, hit);
    json!({
        "id": song.get("id"),
        "title": song.get("title"),
        "artist": song.get("artist"),
        "source": song.get("source"),
        "version": song.get("version"),
        "matched_chart_count": hit.matched_charts.len(),
        "matched_charts": hit.matched_charts.iter().take(10).map(chart_candidate).collect::<Vec<_>>(),
    })
}

fn chart_candidate(chart: &MatchedChart<'_>) -> Value {
    let source = preferred_source(chart);
    json!({
        "source": source.map(|value| value.song.source.key()),
        "chart_type": chart_type(chart.chart.key.generation()),
        "difficulty": difficulty(chart.chart.key.difficulty()),
        "level": chart.chart.level,
        "ds": chart.chart.constant.map(|value| decimal_value(value.value())),
    })
}

fn chart_summary(chart: &MatchedChart<'_>) -> Value {
    let source = preferred_source(chart);
    let notes = chart.chart.notes;
    json!({
        "source": source.map(|value| value.song.source.key()),
        "chart_type": chart_type(chart.chart.key.generation()),
        "difficulty": difficulty(chart.chart.key.difficulty()),
        "level": chart.chart.level,
        "ds": chart.chart.constant.map(|value| decimal_value(value.value())),
        "charter": source.map_or(chart.chart.note_designer.as_str(), |value| value.chart.note_designer.as_str()),
        "version": source.map(|value| value.chart.version.as_str()),
        "original_note_counts": original_note_counts(notes),
        "scoring_note_totals": scoring_note_totals(notes),
        "touch_scored_as_tap": notes.touch > 0,
    })
}

fn preferred_source<'a>(chart: &'a MatchedChart<'_>) -> Option<&'a SourceChartMatch<'a>> {
    chart
        .source_matches
        .iter()
        .min_by_key(|source| source_priority(source.song.source))
}

fn scoring_note_totals(notes: NoteCounts) -> Value {
    json!({
        "tap": notes.tap,
        "touch": notes.touch,
        "hold": notes.hold,
        "slide": notes.slide,
        "break": notes.break_notes,
    })
}

fn original_note_counts(notes: NoteCounts) -> Value {
    json!({
        "tap": notes.tap,
        "hold": notes.hold,
        "slide": notes.slide,
        "touch": notes.touch,
        "break": notes.break_notes,
        "total": notes.total(),
    })
}

#[cfg(test)]
mod tests;
