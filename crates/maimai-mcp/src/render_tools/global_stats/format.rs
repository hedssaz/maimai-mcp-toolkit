use maimai_app::music_global_stats::{
    MusicGlobalStatsBatchResult, MusicGlobalStatsFailureKind, MusicGlobalStatsImage,
    MusicGlobalStatsItemError,
};
use serde_json::{Value, json};

use crate::render_tools::RenderToolError;

pub(super) fn single(result: &MusicGlobalStatsBatchResult) -> Result<String, RenderToolError> {
    if result.images.len() == 1 && result.errors.is_empty() {
        return serde_json::to_string(&image_path(&result.images[0]))
            .map_err(|_| RenderToolError::Serialization);
    }
    batch(result)
}

pub(super) fn single_error(result: &MusicGlobalStatsBatchResult) -> Option<String> {
    if !result.images.is_empty() || result.errors.len() != 1 {
        return None;
    }
    let error = &result.errors[0];
    Some(match error.kind {
        MusicGlobalStatsFailureKind::Input => error.message.clone(),
        MusicGlobalStatsFailureKind::Render => {
            format!("渲染全服统计图失败: {}", error.message)
        }
    })
}

pub(super) fn batch(result: &MusicGlobalStatsBatchResult) -> Result<String, RenderToolError> {
    let results = result.images.iter().map(image_value).collect::<Vec<_>>();
    let mut payload = serde_json::Map::new();
    payload.insert("results".to_owned(), Value::Array(results.clone()));
    if !results.is_empty() {
        payload.insert("images".to_owned(), Value::Array(results));
    }
    if !result.errors.is_empty() {
        payload.insert(
            "errors".to_owned(),
            Value::Array(result.errors.iter().map(error_value).collect()),
        );
    }
    serde_json::to_string(&Value::Object(payload)).map_err(|_| RenderToolError::Serialization)
}

fn image_path(image: &MusicGlobalStatsImage) -> Value {
    json!({
        "imagePath": image.image_path,
        "mimeType": "image/png",
        "width": image.width,
        "height": image.height,
    })
}

fn image_value(image: &MusicGlobalStatsImage) -> Value {
    let mut value = image_path(image);
    if let Some(fields) = value.as_object_mut() {
        fields.insert("index".to_owned(), json!(image.index));
        fields.insert("query".to_owned(), json!(image.query));
        fields.insert("musicId".to_owned(), json!(image.music_id));
        fields.insert("title".to_owned(), json!(image.title));
        fields.insert("chartType".to_owned(), json!(image.chart_type.label()));
        fields.insert("levelIndex".to_owned(), json!(image.level_index));
    }
    value
}

fn error_value(error: &MusicGlobalStatsItemError) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("index".to_owned(), json!(error.index.to_string()));
    value.insert("query".to_owned(), json!(error.query));
    if let Some(chart_type) = error.chart_type {
        value.insert("chartType".to_owned(), json!(chart_type.label()));
    }
    value.insert("message".to_owned(), json!(error.message));
    Value::Object(value)
}
