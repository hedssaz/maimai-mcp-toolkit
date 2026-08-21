mod alias;
mod refresh;
mod scoring;
mod song;
mod value;

use serde_json::Value;

use self::{
    song::song_set,
    value::{array, count, criteria, fmt, truthy},
};
use super::error::CatalogToolError;

pub(super) struct FormatOptions<'a> {
    pub(super) format: Option<&'a str>,
    pub(super) include_raw: bool,
    pub(super) debug: bool,
}

pub(super) fn render(
    tool: &str,
    value: &Value,
    options: FormatOptions<'_>,
) -> Result<String, CatalogToolError> {
    let format = options
        .format
        .map(|value| value.trim().to_ascii_lowercase());
    if options.include_raw || options.debug || format.as_deref() == Some("json") {
        return Ok(serde_json::to_string_pretty(value)?);
    }
    if !matches!(
        format.as_deref(),
        None | Some("") | Some("text") | Some("compact")
    ) {
        return Err(CatalogToolError::input(
            "format must be text, compact, or json",
        ));
    }
    let compact = format.as_deref() == Some("compact");
    Ok(match tool {
        "search_maimai_songs" => song_set("搜索结果", value, compact),
        "random_maimai_songs" => song_set("随机结果", value, compact),
        "list_maimai_songs_by_id" => song_set("ID 列表", value, compact),
        "batch_search_maimai_songs" => batch(value),
        "list_maimai_versions" => versions(value),
        "query_chart_history" => history(value),
        "add_maimai_alias" => alias::added(value),
        "delete_maimai_alias" => alias::deleted(value),
        "list_maimai_aliases" => alias::listed(value),
        "refresh_maimai_sources" => refresh::render(value),
        "refresh_maimai_sources_job_status" => refresh::render_job(value),
        "score_counts" => scoring::score_counts(value),
        "find_score_combinations" => scoring::find_combinations(value),
        "today_maimai" => value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        _ => serde_json::to_string_pretty(value)?,
    })
}

fn batch(value: &Value) -> String {
    let counts = value.get("counts");
    let mut lines = vec![format!(
        "批量搜索结果: 请求 {}，成功 {}，失败 {}",
        count(counts.and_then(|value| value.get("requested"))),
        count(counts.and_then(|value| value.get("success"))),
        count(counts.and_then(|value| value.get("failure")))
    )];
    let items = array(value.get("items"));
    for item in items.iter().take(50).filter(|item| item.is_object()) {
        let key = item
            .get("key")
            .filter(|value| truthy(value))
            .or_else(|| item.get("index"));
        if item.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            lines.push(format!(
                "- {}: 返回 {} / {}",
                fmt(key, "-"),
                count(item.pointer("/result/count")),
                count(item.pointer("/result/total_matches"))
            ));
        } else {
            lines.push(format!(
                "- {}: ERROR {}",
                fmt(key, "-"),
                fmt(item.pointer("/error/message"), "搜索失败")
            ));
        }
    }
    if items.len() > 50 {
        lines.push(format!("... 还有 {} 项未展开", items.len() - 50));
    }
    lines.join("\n")
}

fn versions(value: &Value) -> String {
    let truncated = value
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut lines = vec![
        format!(
            "版本列表: 返回 {} / {}，truncated={}",
            count(value.get("count")),
            count(value.get("total_matches")),
            if truncated { "True" } else { "False" }
        ),
        format!("条件: {}", criteria(value.get("criteria"))),
    ];
    let latest = array(value.get("latest_cn_versions"));
    if !latest.is_empty() {
        let versions = latest
            .iter()
            .map(|value| fmt(Some(value), "-"))
            .collect::<Vec<_>>()
            .join(" / ");
        let years = array(value.get("latest_cn_years"))
            .iter()
            .map(|value| {
                let year = fmt(Some(value), "-");
                format!("{year} / 20{year}")
            })
            .collect::<Vec<_>>()
            .join(" / ");
        lines.push(format!(
            "国服最新版本号: {versions}；按年份查传 version={years}"
        ));
    }
    lines.extend(array(value.get("versions")).iter().map(|item| {
        let sources = array(item.get("sources"))
            .iter()
            .map(|value| fmt(Some(value), "-"))
            .collect::<Vec<_>>()
            .join("/");
        format!(
            "- {} | songs {} | charts {} | sources {}",
            fmt(item.get("version"), "-"),
            fmt(item.get("song_count"), "-"),
            fmt(item.get("chart_count"), "-"),
            if sources.is_empty() { "-" } else { &sources }
        )
    }));
    lines.join("\n")
}

fn history(value: &Value) -> String {
    let songs = array(value.get("songs"));
    if songs.is_empty() {
        return "未找到匹配的歌曲或没有定数变化历史数据。".to_owned();
    }
    let mut lines = Vec::new();
    for song in songs {
        lines.push(format!(
            "{} | 编号 {} | {}",
            fmt(song.get("title"), "-"),
            fmt(song.get("id"), "-"),
            fmt(song.get("artist"), "-")
        ));
        for chart in array(song.get("charts")) {
            lines.push(format!(
                "  {} {} 等级 {} 当前定数 {}",
                fmt(chart.get("chart_type"), "-").to_uppercase(),
                fmt(chart.get("difficulty"), "-"),
                fmt(chart.get("level"), "-"),
                fmt(chart.get("current_ds"), "-")
            ));
            for entry in array(chart.get("history")) {
                let versions = array(entry.get("versions"))
                    .iter()
                    .map(|value| fmt(Some(value), "-"))
                    .collect::<Vec<_>>()
                    .join(" / ");
                lines.push(format!("    {versions}: {}", fmt(entry.get("ds"), "-")));
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
