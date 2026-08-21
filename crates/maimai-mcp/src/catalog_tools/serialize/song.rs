use std::collections::{BTreeMap, BTreeSet};

use maimai_catalog::{
    CatalogSnapshot, MatchKind, RegionAvailability, SearchHit, SourceKind, SourceSongProjection,
};
use serde_json::{Map, Value, json};

use super::{
    chart::{matched_source_charts, source_chart_types},
    decimal_value, match_value, regions, source_id_value, source_priority,
};

pub(super) fn song_value(snapshot: &CatalogSnapshot, hit: &SearchHit<'_>) -> Value {
    let mut projections = hit.metadata.source_projections.iter().collect::<Vec<_>>();
    projections.sort_by_key(|projection| source_priority(projection.source));
    let primary = projections.first().copied();
    let mut output = Map::new();
    let canonical_id = primary.map_or_else(
        || source_id_value(&hit.music.primary_id),
        canonical_source_id,
    );
    output.insert("id".to_owned(), json!(canonical_id));
    output.insert(
        "source_id".to_owned(),
        json!(primary.map_or_else(|| canonical_id.clone(), |value| source_id_value(&value.id))),
    );
    let (source_ids, source_id_aliases) = source_ids(&projections);
    output.insert("source_ids".to_owned(), source_ids);
    if !source_id_aliases.as_object().is_none_or(Map::is_empty) {
        output.insert("source_id_aliases".to_owned(), source_id_aliases);
    }
    output.insert("title".to_owned(), json!(display_title(hit, primary)));
    output.insert(
        "available_chart_types".to_owned(),
        json!(source_chart_types(&projections)),
    );
    output.insert(
        "artist".to_owned(),
        json!(primary.map_or(hit.music.artist.as_str(), |value| value.artist.as_str())),
    );
    output.insert(
        "genre".to_owned(),
        json!(primary.map_or(hit.music.genre.as_str(), |value| value.genre.as_str())),
    );
    output.insert(
        "bpm".to_owned(),
        primary
            .and_then(|value| value.bpm)
            .map_or_else(|| json!(hit.music.bpm), decimal_value),
    );
    output.insert(
        "version".to_owned(),
        json!(primary.map_or(hit.music.version.as_str(), |value| value.version.as_str())),
    );
    output.insert(
        "release_date".to_owned(),
        json!(
            projections
                .iter()
                .filter_map(|value| value.release_date)
                .min()
                .map(|value| value.to_string())
        ),
    );
    output.insert(
        "is_new".to_owned(),
        json!(
            primary
                .and_then(|value| value.is_new)
                .unwrap_or(hit.metadata.is_new_cn || hit.metadata.is_new_jp)
        ),
    );
    output.insert("is_locked".to_owned(), json!(hit.metadata.is_locked));
    let image_name = projections
        .iter()
        .find(|projection| projection.source == SourceKind::Japan)
        .and_then(|projection| projection.image_name.clone());
    output.insert("image_name".to_owned(), json!(image_name));
    output.insert(
        "image_url".to_owned(),
        json!(cover_url(image_name.as_deref())),
    );
    let labels = projections
        .iter()
        .map(|projection| projection.source)
        .collect::<BTreeSet<_>>();
    output.insert(
        "source".to_owned(),
        json!(
            labels
                .iter()
                .map(|source| source.source_name())
                .collect::<Vec<_>>()
                .join("+")
        ),
    );
    output.insert(
        "source_labels".to_owned(),
        json!(labels.iter().map(|source| source.key()).collect::<Vec<_>>()),
    );
    output.insert(
        "primary_source".to_owned(),
        json!(primary.map(|projection| projection.source.key())),
    );
    output.insert("source_fields".to_owned(), source_fields(&projections));
    output.insert("regions".to_owned(), regions(hit.metadata.regions));
    output.insert("levels".to_owned(), json!(all_levels(&projections)));
    output.insert("ds".to_owned(), all_constants(&projections));
    output.insert(
        "aliases".to_owned(),
        json!(
            hit.music
                .aliases
                .iter()
                .map(|value| snapshot.to_simplified(value))
                .collect::<Vec<_>>()
        ),
    );
    output.insert(
        "matched_charts".to_owned(),
        Value::Array(matched_source_charts(snapshot, hit)),
    );
    if hit.matched_by != MatchKind::FilterOnly {
        let value = hit.matched_value.as_deref().unwrap_or(&hit.music.title);
        output.insert(
            "match".to_owned(),
            match_value(hit.matched_by, &snapshot.to_simplified(value)),
        );
    }
    Value::Object(output)
}

fn display_title<'a>(hit: &'a SearchHit<'_>, primary: Option<&'a SourceSongProjection>) -> &'a str {
    primary.map_or(hit.music.title.as_str(), |value| value.title.as_str())
}

fn source_ids(projections: &[&SourceSongProjection]) -> (Value, Value) {
    let mut ids = Map::new();
    let mut aliases = BTreeMap::<String, Vec<String>>::new();
    for projection in projections {
        let key = projection.source.key().to_owned();
        let value = source_id_value(&projection.id);
        if let Some(existing) = ids.get(&key).and_then(Value::as_str) {
            if existing != value {
                let values = aliases.entry(key).or_default();
                if values.is_empty() {
                    values.push(existing.to_owned());
                }
                if !values.contains(&value) {
                    values.push(value);
                }
            }
        } else {
            ids.insert(key, json!(value));
        }
    }
    (Value::Object(ids), json!(aliases))
}

fn source_fields(projections: &[&SourceSongProjection]) -> Value {
    let mut result = Map::new();
    for source in projections {
        let key = source.source.key().to_owned();
        let mut fields = Map::new();
        fields.insert("source".to_owned(), json!(source.source.source_name()));
        fields.insert("id".to_owned(), json!(canonical_source_id(source)));
        fields.insert("source_id".to_owned(), json!(source_id_value(&source.id)));
        fields.insert("title".to_owned(), json!(source.title));
        fields.insert("artist".to_owned(), json!(source.artist));
        fields.insert("genre".to_owned(), json!(source.genre));
        insert_some(&mut fields, "bpm", source.bpm.map(decimal_value));
        if let Some(version) = source.source_fields.numeric_version {
            fields.insert("version".to_owned(), json!(version));
        } else {
            insert_nonempty(&mut fields, "version", &source.version);
        }
        insert_some(
            &mut fields,
            "release_date",
            source.release_date.map(|value| json!(value.to_string())),
        );
        insert_some(
            &mut fields,
            "is_new",
            source.is_new.map(|value| json!(value)),
        );
        insert_some(
            &mut fields,
            "is_locked",
            source.is_locked.map(|value| json!(value)),
        );
        fields.insert(
            "available_chart_types".to_owned(),
            json!(source_chart_types(&[*source])),
        );
        fields.insert("levels".to_owned(), json!(ordered_levels(source)));
        fields.insert("ds".to_owned(), Value::Array(ordered_constants(source)));
        let mut aggregate = RegionAvailability::default();
        for chart in &source.charts {
            aggregate.merge(chart.regions);
        }
        fields.insert("regions".to_owned(), regions(aggregate));
        insert_some(
            &mut fields,
            "release_version",
            source
                .source_fields
                .release_version
                .map(|value| json!(value)),
        );
        insert_string(
            &mut fields,
            "official_add_version",
            source.source_fields.official_add_version.as_deref(),
        );
        insert_string(
            &mut fields,
            "category",
            source.source_fields.category.as_deref(),
        );
        insert_string(
            &mut fields,
            "asset_dir",
            source.source_fields.asset_dir.as_deref(),
        );
        insert_string(
            &mut fields,
            "jacket_path",
            source.source_fields.jacket_path.as_deref(),
        );
        insert_string(
            &mut fields,
            "rights",
            source.source_fields.rights.as_deref(),
        );
        insert_string(&mut fields, "map", source.source_fields.map.as_deref());
        insert_string(&mut fields, "slug", source.source_fields.slug.as_deref());
        insert_string(
            &mut fields,
            "keyword",
            source.source_fields.keyword.as_deref(),
        );
        insert_string(
            &mut fields,
            "comment",
            source.source_fields.comment.as_deref(),
        );
        if let Some(current) = result.get_mut(&key).and_then(Value::as_object_mut) {
            merge_source_fields(current, &fields);
        } else {
            result.insert(key, Value::Object(fields));
        }
    }
    Value::Object(result)
}

fn merge_source_fields(current: &mut Map<String, Value>, incoming: &Map<String, Value>) {
    for (key, value) in incoming {
        if matches!(key.as_str(), "available_chart_types" | "levels" | "ds") {
            let Some(values) = value.as_array() else {
                continue;
            };
            let target = current
                .entry(key.clone())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(target) = target.as_array_mut() {
                for value in values {
                    if !target.contains(value) {
                        target.push(value.clone());
                    }
                }
            }
        } else if key == "regions" {
            let Some(values) = value.as_object() else {
                continue;
            };
            let target = current
                .entry(key.clone())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(target) = target.as_object_mut() {
                for (region, available) in values {
                    let merged = target.get(region).and_then(Value::as_bool).unwrap_or(false)
                        || available.as_bool().unwrap_or(false);
                    target.insert(region.clone(), json!(merged));
                }
            }
        } else if current
            .get(key)
            .is_none_or(|existing| existing.is_null() || existing == "" || existing == &json!([]))
        {
            current.insert(key.clone(), value.clone());
        }
    }
}

fn all_levels(projections: &[&SourceSongProjection]) -> Vec<String> {
    let mut result = Vec::new();
    for source in projections {
        for value in ordered_levels(source) {
            if !result.contains(&value) {
                result.push(value);
            }
        }
    }
    result
}

fn all_constants(projections: &[&SourceSongProjection]) -> Value {
    let mut values = Vec::new();
    for source in projections {
        for value in source.charts.iter().filter_map(|chart| chart.constant) {
            if !values.contains(&value) {
                values.push(value);
            }
        }
    }
    Value::Array(values.into_iter().map(decimal_value).collect())
}

fn ordered_levels(source: &SourceSongProjection) -> Vec<String> {
    let mut values = Vec::new();
    for chart in &source.charts {
        if !chart.level.is_empty() && !values.contains(&chart.level) {
            values.push(chart.level.clone());
        }
    }
    values
}

fn ordered_constants(source: &SourceSongProjection) -> Vec<Value> {
    let mut values = Vec::new();
    for value in source.charts.iter().filter_map(|chart| chart.constant) {
        if !values.contains(&value) {
            values.push(value);
        }
    }
    values.into_iter().map(decimal_value).collect()
}

fn canonical_source_id(source: &SourceSongProjection) -> String {
    match (source.id.value(), source.source) {
        (maimai_core::SongIdValue::Numeric(value), SourceKind::DivingFish)
            if (10_000..100_000).contains(value)
                && source
                    .charts
                    .iter()
                    .any(|chart| chart.generation == maimai_core::ChartGeneration::Deluxe) =>
        {
            (*value - 10_000).to_string()
        }
        _ => source_id_value(&source.id),
    }
}

fn cover_url(image_name: Option<&str>) -> Option<String> {
    let image_name = image_name?.trim();
    if image_name.is_empty() {
        return None;
    }
    let filename = image_name.rsplit('/').next().unwrap_or(image_name);
    let suffix = if filename.contains('.') { "" } else { ".jpg" };
    Some(format!(
        "https://shama.dxrating.net/images/cover/v2/{image_name}{suffix}"
    ))
}

fn insert_some(output: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        output.insert(key.to_owned(), value);
    }
}

fn insert_nonempty(output: &mut Map<String, Value>, key: &str, value: &str) {
    if !value.is_empty() {
        output.insert(key.to_owned(), json!(value));
    }
}

fn insert_string(output: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        output.insert(key.to_owned(), json!(value));
    }
}
