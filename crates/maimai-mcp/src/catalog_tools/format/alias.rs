use serde_json::Value;

use super::value::{array, count, criteria, fmt};

pub(super) fn added(value: &Value) -> String {
    let status = if value
        .get("existed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "已存在"
    } else {
        "已新增"
    };
    if let Some(label) = name_label(value) {
        let mut lines = vec![
            format!(
                "{label}别名{status}: {} -> {}",
                fmt(value.get("alias"), "-"),
                fmt(value.get("canonical"), "-")
            ),
            format!(
                "该 {label} 当前别名: {}",
                joined(value.get("aliases"), ", ", "无")
            ),
            format!("保存位置: {}", fmt(value.get("document"), "-")),
        ];
        if let Some(warning) = value.get("warning").and_then(Value::as_str) {
            lines.push(format!("⚠ {warning}"));
        }
        return lines.join("\n");
    }
    let mut lines = vec![
        format!(
            "别名{status}: {} -> {} (ID {})",
            fmt(value.get("alias"), "-"),
            fmt(value.get("title"), "-"),
            fmt(value.get("song_id"), "-")
        ),
        format!("保存位置: {}", fmt(value.get("document"), "-")),
    ];
    if let Some(warning) = value.get("warning").and_then(Value::as_str) {
        lines.push(format!("⚠ {warning}"));
    }
    lines.join("\n")
}

pub(super) fn deleted(value: &Value) -> String {
    if let Some(label) = name_label(value) {
        return format!(
            "已删除{label}别名: {} ({label}: {})\n剩余别名: {}\n保存位置: {}",
            fmt(value.get("removed_alias"), "-"),
            fmt(value.get("canonical"), "-"),
            joined(value.get("remaining_aliases"), ", ", "无"),
            fmt(value.get("document"), "-")
        );
    }
    format!(
        "已删除别名: {} (歌曲: {}, ID {})\n剩余别名: {}\n保存位置: {}",
        fmt(value.get("removed_alias"), "-"),
        fmt(value.get("title"), "-"),
        fmt(value.get("song_id"), "-"),
        joined(value.get("remaining_aliases"), ", ", "无"),
        fmt(value.get("document"), "-")
    )
}

pub(super) fn listed(value: &Value) -> String {
    if let Some(label) = name_label(value) {
        let entries = array(value.get("entries"));
        let mut lines = vec![format!(
            "{label}别名词典: 共 {} 条 (保存位置: {})",
            count(value.get("count")),
            fmt(value.get("document"), "-")
        )];
        if entries.is_empty() {
            lines.push(format!("暂无{label}别名。"));
        }
        lines.extend(entries.iter().enumerate().map(|(index, entry)| {
            format!(
                "{}. {} → {}",
                index + 1,
                fmt(entry.get("canonical"), "-"),
                joined(entry.get("aliases"), ", ", "无")
            )
        }));
        return lines.join("\n");
    }
    let truncated = value
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let songs = array(value.get("songs"));
    let mut lines = vec![
        format!(
            "别名列表: 返回 {} / {}，truncated={}",
            count(value.get("count")),
            count(value.get("total_matches")),
            if truncated { "True" } else { "False" }
        ),
        format!("条件: {}", criteria(value.get("criteria"))),
    ];
    if songs.is_empty() {
        lines.push("没有匹配歌曲。".to_owned());
        return lines.join("\n");
    }
    for (index, song) in songs.iter().enumerate() {
        let aliases = array(song.get("aliases"));
        lines.push(format!(
            "{}. {} | ID {} | source {} | 别名数 {}",
            index + 1,
            fmt(song.get("title"), "-"),
            fmt(song.get("id"), "-"),
            fmt(song.get("source"), "-"),
            aliases.len()
        ));
        lines.push(format!(
            "  {}",
            joined(song.get("aliases"), " / ", "无别名")
        ));
    }
    lines.join("\n")
}

fn name_label(value: &Value) -> Option<&'static str> {
    match value.get("kind").and_then(Value::as_str) {
        Some("artist") => Some("曲师"),
        Some("charter") => Some("谱师"),
        _ => None,
    }
}

fn joined(value: Option<&Value>, separator: &str, fallback: &str) -> String {
    let value = array(value)
        .iter()
        .map(|value| fmt(Some(value), "-"))
        .collect::<Vec<_>>()
        .join(separator);
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}
