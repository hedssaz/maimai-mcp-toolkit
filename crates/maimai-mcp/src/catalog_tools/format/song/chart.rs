use std::collections::BTreeSet;

use serde_json::Value;

use super::super::value::{
    array, chart_source, chart_type, fmt, format_ds, numeric, present, source_label, source_rank,
    type_order,
};

pub(super) fn full_chart(song: &Value, chart: &Value) -> String {
    let source = chart_source(chart.get("source"));
    let kind = chart_type(chart.get("chart_type"));
    let id = chart_id(song, chart);
    let type_id = if id.is_empty() {
        kind
    } else {
        format!("{kind}#{id}")
    };
    let mut fit = format!(
        "拟合 {}, 差值 {}",
        format_ds(chart.get("fit_diff")),
        fmt(chart.get("fit_delta"), "-")
    );
    let label = fmt(chart.get("fit_label"), "");
    if !label.is_empty() {
        fit.push_str(&format!(", {label}"));
    }
    let extras = chart_extras(chart);
    let tail = if extras.is_empty() {
        String::new()
    } else {
        format!(" | {}", extras.join(" "))
    };
    format!(
        "- {source} {type_id} {} 等级 {} 定数 {} | {fit} | {} | 谱师 {}{tail}",
        fmt(chart.get("difficulty"), "-"),
        fmt(chart.get("level"), "-"),
        format_ds(chart.get("ds")),
        notes(chart.get("notes")),
        fmt(chart.get("charter"), "-")
    )
}

pub(super) fn compact_chart(chart: &Value) -> String {
    let difficulty = fmt(chart.get("difficulty"), "");
    let short = match difficulty.as_str() {
        "Basic" => "Bas",
        "Advanced" => "Adv",
        "Expert" => "Exp",
        "Master" => "Mst",
        "Re:MASTER" => "ReM",
        _ => difficulty.get(..3).unwrap_or(&difficulty),
    };
    let level = fmt(chart.get("level"), "-");
    let constant = format_ds(chart.get("ds"));
    let head = if constant == "-" {
        level
    } else {
        format!("{level}/{constant}")
    };
    let fit = chart
        .get("fit_diff")
        .and_then(Value::as_f64)
        .map_or_else(String::new, |value| {
            let label = chart
                .get("fit_label")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map_or(String::new(), |value| format!(" {value}"));
            format!(" (拟合 {value:.2}{label})")
        });
    format!("{short} {head}{fit}")
}

pub(super) fn source_differences(song: &Value) -> String {
    let Some(fields) = song
        .get("source_fields")
        .and_then(Value::as_object)
        .filter(|fields| fields.len() >= 2)
    else {
        return String::new();
    };
    let mut pieces = Vec::new();
    for key in ["version", "genre", "release_date", "is_new", "ds"] {
        let mut values = Vec::new();
        for source in ["cn", "official", "divingfish", "jp"] {
            let Some(field) = fields.get(source).and_then(Value::as_object) else {
                continue;
            };
            let rendered = if key == "ds" {
                let items = array(field.get(key));
                if items.is_empty() {
                    continue;
                }
                items
                    .iter()
                    .map(|value| format_ds(Some(value)))
                    .collect::<Vec<_>>()
                    .join("/")
            } else {
                if !present(field.get(key)) {
                    continue;
                }
                fmt(field.get(key), "-")
            };
            values.push((source, rendered));
        }
        if values.len() < 2 || values.iter().all(|(_, value)| value == &values[0].1) {
            continue;
        }
        let field_label = match key {
            "version" => "版本",
            "genre" => "流派",
            "release_date" => "日服上线",
            "is_new" => "新曲",
            _ => "定数",
        };
        let details = values
            .into_iter()
            .map(|(source, value)| format!("{} {value}", source_label(source)))
            .collect::<Vec<_>>()
            .join(", ");
        pieces.push(format!("{field_label}: {details}"));
    }
    if pieces.is_empty() {
        String::new()
    } else {
        format!("源差异: {}", pieces.join("；"))
    }
}

pub(super) fn song_id(song: &Value, compact: bool) -> String {
    let mut ids: Vec<(String, String)> = Vec::new();
    for chart in dedupe(array(song.get("matched_charts"))) {
        let kind = chart_type(chart.get("chart_type"));
        if ids.iter().any(|(existing, _)| existing == &kind) {
            continue;
        }
        let id = chart_id(song, chart);
        if !id.is_empty() {
            ids.push((kind, id));
        }
    }
    ids.sort_by_key(|(kind, _)| type_order(kind));
    let unique = ids.iter().map(|(_, id)| id).collect::<BTreeSet<_>>();
    if ids.is_empty() {
        let id = fmt(song.get("id"), "-");
        return if compact { format!("#{id}") } else { id };
    }
    if unique.len() == 1 {
        return if compact {
            format!("#{}", ids[0].1)
        } else {
            ids[0].1.clone()
        };
    }
    if compact {
        format!(
            "ID {}",
            ids.iter()
                .map(|(kind, id)| format!("{kind}#{id}"))
                .collect::<Vec<_>>()
                .join(" / ")
        )
    } else {
        ids.iter()
            .map(|(kind, id)| format!("{kind} {id}"))
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

pub(super) fn chart_id(song: &Value, chart: &Value) -> String {
    let kind = chart_type(chart.get("chart_type"));
    for key in ["chart_id", "music_id", "musicId", "internal_id"] {
        if let Some(id) = numeric(chart.get(key))
            && (kind != "DX" || id > 10_000)
        {
            return id.to_string();
        }
    }
    let Some(id) = numeric(song.get("id")) else {
        return String::new();
    };
    if kind == "DX" && id <= 10_000 {
        (id + 10_000).to_string()
    } else {
        id.to_string()
    }
}

pub(super) fn dedupe(charts: &[Value]) -> Vec<&Value> {
    let mut best: Vec<&Value> = Vec::new();
    for chart in charts.iter().filter(|chart| chart.is_object()) {
        let key = (
            fmt(chart.get("chart_type"), ""),
            numeric(chart.get("difficulty_index")).unwrap_or(0),
        );
        if let Some(index) = best.iter().position(|old| {
            (
                fmt(old.get("chart_type"), ""),
                numeric(old.get("difficulty_index")).unwrap_or(0),
            ) == key
        }) {
            let ranks =
                source_rank(chart.get("source")).zip(source_rank(best[index].get("source")));
            if ranks.is_some_and(|(new, old)| new < old) {
                best[index] = chart;
            }
        } else {
            best.push(chart);
        }
    }
    best.sort_by_key(|chart| {
        (
            type_order(&chart_type(chart.get("chart_type"))),
            numeric(chart.get("difficulty_index")).unwrap_or(0),
        )
    });
    best
}

fn chart_extras(chart: &Value) -> Vec<String> {
    let mut extras = Vec::new();
    if present(chart.get("version")) {
        extras.push(format!("版本 {}", fmt(chart.get("version"), "-")));
    }
    if chart.get("is_buddy") == Some(&Value::Bool(true)) {
        extras.push("双人谱".to_owned());
    }
    if present(chart.get("kanji")) {
        extras.push(format!("字标 {}", fmt(chart.get("kanji"), "-")));
    }
    if let Some(value) = chart
        .get("description")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        let shortened = if value.chars().count() <= 40 {
            value.to_owned()
        } else {
            format!("{}…", value.chars().take(40).collect::<String>())
        };
        extras.push(format!("说明 {shortened}"));
    }
    extras
}

fn notes(value: Option<&Value>) -> String {
    fn totals(value: Option<&Value>) -> [i64; 6] {
        let Some(object) = value.and_then(Value::as_object) else {
            return [0; 6];
        };
        if object.get("left").is_some_and(Value::is_object)
            || object.get("right").is_some_and(Value::is_object)
        {
            let left = totals(object.get("left"));
            let right = totals(object.get("right"));
            return std::array::from_fn(|index| left[index] + right[index]);
        }
        let mut result = ["total", "tap", "hold", "slide", "touch", "break"].map(|key| {
            object.get(key).map_or(0, |value| {
                value.as_i64().unwrap_or_else(|| {
                    value.as_f64().map_or(0, |number| {
                        if number.is_finite()
                            && number >= i64::MIN as f64
                            && number <= i64::MAX as f64
                        {
                            number.trunc() as i64
                        } else {
                            0
                        }
                    })
                })
            })
        });
        if result[0] == 0 {
            result[0] = result[1..].iter().sum();
        }
        result
    }
    let notes = totals(value);
    if notes.iter().all(|value| *value == 0) {
        "-".to_owned()
    } else {
        format!(
            "合计 {} (Tap {}, Hold {}, Slide {}, Touch {}, Break {})",
            notes[0], notes[1], notes[2], notes[3], notes[4], notes[5]
        )
    }
}
