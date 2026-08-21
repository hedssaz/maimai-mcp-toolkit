use maimai_app::music_score::{MusicScoreBatchResult, MusicScoreImage, MusicScoreItemError};
use maimai_core::ScoreSource;
use serde_json::{Map, Value, json};

use super::handler::MusicScoreSurface;
use crate::render_tools::{RenderToolError, score_source};

pub(super) fn result(
    result: &MusicScoreBatchResult,
    surface: MusicScoreSurface,
) -> Result<String, RenderToolError> {
    if result.images.len() == 1 && result.errors.is_empty() {
        let mut value = image_path(&result.images[0]);
        apply_source(&mut value, result.source, surface);
        return stringify(value);
    }
    batch(result, surface)
}

pub(super) fn batch(
    result: &MusicScoreBatchResult,
    surface: MusicScoreSurface,
) -> Result<String, RenderToolError> {
    let images = result
        .images
        .iter()
        .map(|image| image_value(image, result.source, surface))
        .collect::<Vec<_>>();
    let mut payload = Map::new();
    payload.insert("results".to_owned(), Value::Array(images.clone()));
    if !images.is_empty() {
        payload.insert("images".to_owned(), Value::Array(images));
    }
    if !result.errors.is_empty() {
        payload.insert(
            "errors".to_owned(),
            Value::Array(result.errors.iter().map(error_value).collect()),
        );
    }
    apply_source_object(&mut payload, result.source, surface);
    stringify(Value::Object(payload))
}

fn image_path(image: &MusicScoreImage) -> Value {
    json!({
        "imagePath":image.image_path,
        "mimeType":"image/png",
        "width":image.width,
        "height":image.height,
    })
}

fn image_value(image: &MusicScoreImage, source: ScoreSource, surface: MusicScoreSurface) -> Value {
    let mut value = image_path(image);
    if let Some(fields) = value.as_object_mut() {
        fields.insert("index".to_owned(), json!(image.index));
        fields.insert("query".to_owned(), json!(image.query));
        fields.insert("musicId".to_owned(), json!(image.music_id));
        fields.insert("title".to_owned(), json!(image.title));
        fields.insert("chartType".to_owned(), json!(image.chart_type));
        apply_source_object(fields, source, surface);
    }
    value
}

fn error_value(error: &MusicScoreItemError) -> Value {
    let mut value = Map::new();
    value.insert("index".to_owned(), json!(error.index.to_string()));
    value.insert("query".to_owned(), json!(error.query));
    if let Some(chart_type) = &error.chart_type {
        value.insert("chartType".to_owned(), json!(chart_type));
    }
    value.insert("message".to_owned(), json!(error.message));
    Value::Object(value)
}

fn apply_source(value: &mut Value, source: ScoreSource, surface: MusicScoreSurface) {
    if let Some(fields) = value.as_object_mut() {
        apply_source_object(fields, source, surface);
    }
}

fn apply_source_object(
    fields: &mut Map<String, Value>,
    source: ScoreSource,
    surface: MusicScoreSurface,
) {
    if surface == MusicScoreSurface::Public {
        return;
    }
    score_source::insert(fields, source);
}

fn stringify(value: Value) -> Result<String, RenderToolError> {
    serde_json::to_string(&value).map_err(|_| RenderToolError::Serialization)
}
