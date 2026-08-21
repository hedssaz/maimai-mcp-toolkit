use std::collections::{BTreeMap, BTreeSet};

use maimai_catalog::{
    CatalogQuery, CatalogSnapshot, CatalogSort, SearchHit, SortDirection, SortKey, SourceKind,
};
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};

use super::{
    convert::catalog_query,
    dto::{HistoryArgs, ListByIdArgs, SearchArgs, VersionsArgs},
    error::CatalogToolError,
    search_ops::{execute_query, search_result},
    serialize::{chart_type, decimal_value, difficulty, source_id_value},
};

pub(super) fn list_by_id(
    snapshot: &CatalogSnapshot,
    arguments: &ListByIdArgs,
) -> Result<Value, CatalogToolError> {
    if arguments.limit == 0 || arguments.limit > 2_000 {
        return Err(CatalogToolError::input(
            "count/limit must be between 1 and 2000",
        ));
    }
    let direction = match arguments.order.trim().to_ascii_lowercase().as_str() {
        "asc" | "ascending" | "正序" | "升序" => SortDirection::Ascending,
        "desc" | "descending" | "倒序" | "降序" => SortDirection::Descending,
        _ => return Err(CatalogToolError::input("order must be asc or desc")),
    };
    let mut query = catalog_query(snapshot, &arguments.search)?;
    query.limit = None;
    if arguments.search.sort.is_none() {
        query.sort = CatalogSort {
            key: SortKey::Id,
            direction,
        };
    }
    let mut hits = snapshot.query(&query)?;
    let total = hits.len();
    hits.truncate(arguments.limit);
    let mut value = search_result(
        snapshot,
        &arguments.search,
        &hits,
        total,
        total > arguments.limit,
    )?;
    if let Some(criteria) = value.get_mut("criteria").and_then(Value::as_object_mut) {
        criteria.insert(
            "order".to_owned(),
            json!(if direction == SortDirection::Ascending {
                "asc"
            } else {
                "desc"
            }),
        );
        criteria.insert("limit".to_owned(), json!(arguments.limit));
    }
    Ok(value)
}

pub(super) fn versions(
    snapshot: &CatalogSnapshot,
    arguments: &VersionsArgs,
) -> Result<Value, CatalogToolError> {
    if arguments.limit == Some(0) || arguments.limit.is_some_and(|limit| limit > 5_000) {
        return Err(CatalogToolError::input(
            "count/limit must be between 1 and 5000",
        ));
    }
    let hits = snapshot.query(&CatalogQuery::default())?;
    let mut versions = BTreeMap::<String, VersionCounts>::new();
    for hit in &hits {
        for source in &hit.metadata.source_projections {
            let mut song_seen = BTreeSet::new();
            let sources = BTreeSet::from([source.source]);
            let song_version = source
                .source_fields
                .numeric_version
                .map_or_else(|| source.version.clone(), |value| value.to_string());
            if version_matches(&song_version, arguments.query.as_deref()) {
                let first_for_song = song_seen.insert(song_version.clone());
                add_version(&mut versions, &song_version, &sources, first_for_song);
            }
            for chart in &source.charts {
                if !version_matches(&chart.version, arguments.query.as_deref()) {
                    continue;
                }
                let first_for_song = song_seen.insert(chart.version.clone());
                add_version(&mut versions, &chart.version, &sources, first_for_song);
                if let Some(value) = versions.get_mut(&chart.version) {
                    value.chart_count += 1;
                }
            }
        }
    }
    if let Some(query) = &arguments.query {
        let query = query.to_ascii_lowercase();
        versions.retain(|version, _| version.to_ascii_lowercase().contains(&query));
    }
    let total = versions.len();
    let limit = arguments.limit.unwrap_or(total);
    let mut version_items = versions.into_iter().collect::<Vec<_>>();
    version_items.sort_by(|(left, _), (right, _)| {
        match (left.parse::<i64>(), right.parse::<i64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()),
        }
    });
    let output = version_items
        .into_iter()
        .take(limit)
        .map(|(version, counts)| {
            json!({
                "version": version,
                "song_count": counts.song_count,
                "chart_count": counts.chart_count,
                "sources": counts.sources,
            })
        })
        .collect::<Vec<_>>();
    let mut result = Map::new();
    result.insert("count".to_owned(), json!(output.len()));
    result.insert("total_matches".to_owned(), json!(total));
    if output.len() < total {
        result.insert("truncated".to_owned(), json!(true));
    }
    result.insert(
        "criteria".to_owned(),
        json!({"query": arguments.query, "limit": arguments.limit}),
    );
    result.insert(
        "latest_cn_versions".to_owned(),
        json!(snapshot.latest_cn_versions()),
    );
    result.insert(
        "latest_cn_years".to_owned(),
        json!(
            snapshot
                .latest_cn_versions()
                .iter()
                .map(|version| version.to_string().chars().take(2).collect::<String>())
                .collect::<Vec<_>>()
        ),
    );
    result.insert("versions".to_owned(), Value::Array(output));
    Ok(Value::Object(result))
}

fn version_matches(value: &str, query: Option<&str>) -> bool {
    !value.is_empty()
        && query.is_none_or(|query| {
            value
                .to_ascii_lowercase()
                .contains(&query.to_ascii_lowercase())
        })
}

pub(super) fn history(
    snapshot: &CatalogSnapshot,
    arguments: &HistoryArgs,
) -> Result<Value, CatalogToolError> {
    if arguments.query.trim().is_empty() {
        return Err(CatalogToolError::input("query is required"));
    }
    let search = SearchArgs {
        query: Some(arguments.query.clone()),
        difficulty: arguments.difficulty.clone(),
        song_type: arguments.song_type.clone(),
        limit: Some(20),
        ..SearchArgs::default()
    };
    let (_, hits) = execute_query(snapshot, &search)?;
    let songs = hits
        .iter()
        .filter_map(|hit| history_song(snapshot, hit))
        .collect::<Vec<_>>();
    Ok(json!({"count": songs.len(), "songs": songs}))
}

fn history_song(snapshot: &CatalogSnapshot, hit: &SearchHit<'_>) -> Option<Value> {
    let charts = hit
        .matched_charts
        .iter()
        .filter(|chart| !chart.metadata.multiver_constants.is_empty())
        .map(|chart| {
            json!({
                "chart_type": chart_type(chart.chart.key.generation()),
                "difficulty": difficulty(chart.chart.key.difficulty()),
                "level": chart.chart.level,
                "current_ds": chart.chart.constant.map(|value| decimal_value(value.value())),
                "history": collapse_history(
                    snapshot.version_order(),
                    &chart.metadata.multiver_constants,
                ),
            })
        })
        .collect::<Vec<_>>();
    if charts.is_empty() {
        return None;
    }
    Some(json!({
        "title": hit.music.title,
        "id": source_id_value(&hit.music.primary_id),
        "artist": hit.music.artist,
        "charts": charts,
    }))
}

fn collapse_history(order: &[String], values: &BTreeMap<String, Decimal>) -> Vec<Value> {
    let mut ordered = order
        .iter()
        .filter_map(|version| values.get(version).map(|value| (version, value)))
        .collect::<Vec<_>>();
    for (version, value) in values {
        if !order.contains(version) {
            ordered.push((version, value));
        }
    }
    let mut groups: Vec<(Vec<&str>, Decimal)> = Vec::new();
    for (version, value) in ordered {
        if let Some((versions, previous)) = groups.last_mut()
            && *previous == *value
        {
            versions.push(version);
        } else {
            groups.push((vec![version], *value));
        }
    }
    groups
        .into_iter()
        .map(|(versions, value)| json!({"versions": versions, "ds": decimal_value(value)}))
        .collect()
}

#[derive(Default)]
struct VersionCounts {
    song_count: usize,
    chart_count: usize,
    sources: BTreeSet<String>,
}

fn add_version(
    versions: &mut BTreeMap<String, VersionCounts>,
    version: &str,
    sources: &BTreeSet<SourceKind>,
    song: bool,
) {
    if version.is_empty() {
        return;
    }
    let value = versions.entry(version.to_owned()).or_default();
    if song {
        value.song_count += 1;
    }
    value
        .sources
        .extend(sources.iter().map(|source| source.source_name().to_owned()));
}
