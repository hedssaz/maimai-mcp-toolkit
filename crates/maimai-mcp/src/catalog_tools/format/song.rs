mod chart;

use serde_json::Value;

use self::chart::{compact_chart, dedupe, full_chart, song_id, source_differences};
use super::value::{
    array, chart_type, chart_type_list, criteria, fmt, present, regions, song_sources,
};

pub(super) fn song_set(title: &str, value: &Value, compact: bool) -> String {
    let total = value
        .get("total_matches")
        .or_else(|| value.get("total_candidates"));
    let mut lines = vec![format!(
        "{title}: 返回 {} / {}",
        super::value::count(value.get("count")),
        super::value::count(total)
    )];
    if value.get("truncated") == Some(&Value::Bool(true)) {
        lines[0].push_str("，已截断（加 limit 看更多）");
    }
    lines.push(format!("条件: {}", criteria(value.get("criteria"))));
    let songs = array(value.get("songs"));
    if songs.is_empty() {
        lines.push("没有匹配歌曲。".to_owned());
    } else {
        lines.extend(songs.iter().enumerate().map(|(index, song)| {
            if compact {
                compact_song(song, index + 1)
            } else {
                full_song(song, index + 1)
            }
        }));
    }
    lines.join("\n")
}

fn full_song(song: &Value, index: usize) -> String {
    let mut lines = vec![format!(
        "{index}. {} | 编号 {} | {} | 来源 {} | 地区 {} | 版本 {} | BPM {}",
        fmt(song.get("title"), "-"),
        song_id(song, false),
        fmt(song.get("artist"), "-"),
        song_sources(song.get("source")),
        regions(song.get("regions")),
        fmt(song.get("version"), "-"),
        fmt(song.get("bpm"), "-")
    )];
    let mut meta = Vec::new();
    if present(song.get("release_date")) {
        meta.push(format!("日服上线 {}", fmt(song.get("release_date"), "-")));
    }
    if song.get("is_new") == Some(&Value::Bool(true)) {
        meta.push("新曲".to_owned());
    }
    if present(song.get("genre")) {
        meta.push(format!("流派 {}", fmt(song.get("genre"), "-")));
    }
    let types = chart_type_list(song.get("available_chart_types"));
    if !types.is_empty() {
        meta.push(format!("谱面 {types}"));
    }
    let matched = match_text(song.get("match"));
    if !matched.is_empty() {
        meta.push(matched);
    }
    if !meta.is_empty() {
        lines.push(format!("  {}", meta.join(" | ")));
    }
    let aliases = aliases(song.get("aliases"), 8, true);
    if !aliases.is_empty() {
        lines.push(format!("  {aliases}"));
    }
    let differences = source_differences(song);
    if !differences.is_empty() {
        lines.push(format!("  {differences}"));
    }
    let charts = array(song.get("matched_charts"));
    lines.extend(
        charts
            .iter()
            .take(12)
            .map(|chart| format!("  {}", full_chart(song, chart))),
    );
    if charts.len() > 12 {
        lines.push(format!("  ... 还有 {} 张谱面未展开", charts.len() - 12));
    }
    lines.join("\n")
}

fn compact_song(song: &Value, index: usize) -> String {
    let types = chart_type_list(song.get("available_chart_types"));
    let type_suffix = if types.is_empty() {
        String::new()
    } else {
        format!(" | 谱面 {types}")
    };
    let mut head = format!(
        "{index}. {} | {} | {} | {} BPM | v{}{type_suffix}",
        fmt(song.get("title"), "-"),
        song_id(song, true),
        fmt(song.get("artist"), "-"),
        fmt(song.get("bpm"), "-"),
        fmt(song.get("version"), "-")
    );
    let matched = match_text(song.get("match"));
    if !matched.is_empty() {
        head.push_str(&format!(" | {matched}"));
    }
    let mut flags = Vec::new();
    if song.get("is_new") == Some(&Value::Bool(true)) {
        flags.push("新曲");
    }
    if song.get("is_locked") == Some(&Value::Bool(true)) {
        flags.push("锁定");
    }
    if !flags.is_empty() {
        head.push_str(&format!("  [{}]", flags.join(" ")));
    }
    let mut lines = vec![head];
    let charts = dedupe(array(song.get("matched_charts")));
    for kind in ["ST", "DX"] {
        let group = charts
            .iter()
            .copied()
            .filter(|chart| chart_type(chart.get("chart_type")) == kind)
            .collect::<Vec<_>>();
        if let Some(first) = group.first() {
            let id = chart::chart_id(song, first);
            let label = if id.is_empty() {
                kind.to_owned()
            } else {
                format!("{kind} #{id}")
            };
            lines.push(format!(
                "  {label}: {}",
                group
                    .iter()
                    .map(|chart| compact_chart(chart))
                    .collect::<Vec<_>>()
                    .join(" / ")
            ));
        }
    }
    let alias_line = aliases(song.get("aliases"), 5, false);
    if !alias_line.is_empty() {
        lines.push(format!("  {alias_line}"));
    }
    lines.join("\n")
}

fn aliases(value: Option<&Value>, limit: usize, deduplicate: bool) -> String {
    let mut values = Vec::new();
    for alias in array(value)
        .iter()
        .map(|value| fmt(Some(value), ""))
        .filter(|value| !value.trim().is_empty())
    {
        if !deduplicate || !values.contains(&alias) {
            values.push(alias);
        }
    }
    if values.is_empty() {
        return String::new();
    }
    let suffix = if values.len() > limit {
        format!(" (+{})", values.len() - limit)
    } else {
        String::new()
    };
    format!(
        "别名: {}{suffix}",
        values[..values.len().min(limit)].join(" / ")
    )
}

fn match_text(value: Option<&Value>) -> String {
    let Some(value) = value.and_then(Value::as_object) else {
        return String::new();
    };
    let field = value.get("field").and_then(Value::as_str).unwrap_or("");
    if matches!(field, "" | "none" | "unknown") {
        return String::new();
    }
    let raw_label = value.get("label").and_then(Value::as_str).unwrap_or("");
    let label = match field {
        "song_id" => "歌曲ID",
        "source_id" => "源ID",
        "title" => "歌名",
        "alias" => "别名",
        "pinyin" => "拼音",
        "keyword" => "关键字",
        _ => raw_label.strip_suffix("命中").unwrap_or(raw_label),
    };
    if label.is_empty() {
        return String::new();
    }
    let mode = match value.get("mode").and_then(Value::as_str) {
        Some("exact") => "(精确)",
        Some("prefix") => "(前缀)",
        Some("contains") => "(包含)",
        _ => "",
    };
    let matched = fmt(value.get("value"), "").trim().to_owned();
    let suffix = if matched.is_empty() {
        String::new()
    } else {
        format!(": {matched}")
    };
    format!("命中 {label}{mode}{suffix}")
}
