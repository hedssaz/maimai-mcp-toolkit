use maimai_app::music_info::{MusicInfoBatchResult, MusicInfoImage, MusicInfoItemError};
use serde_json::{Value, json};

use super::RenderToolError;

pub(super) fn single(result: &MusicInfoBatchResult) -> Result<String, RenderToolError> {
    if result.images.len() == 1 && result.errors.is_empty() {
        return serde_json::to_string(&image_path(&result.images[0]))
            .map_err(|_| RenderToolError::Serialization);
    }
    batch_value(result, false)
}

pub(super) fn batch(result: &MusicInfoBatchResult) -> Result<String, RenderToolError> {
    batch_value(result, true)
}

pub(super) fn single_error(result: &MusicInfoBatchResult) -> Option<String> {
    if !result.images.is_empty() || result.errors.len() != 1 {
        return None;
    }
    let message = &result.errors[0].message;
    Some(
        if message.starts_with("未找到曲目")
            || message.starts_with("匹配到多个曲目")
            || message.starts_with("需要提供")
        {
            message.clone()
        } else {
            format!("渲染曲目信息失败: {message}")
        },
    )
}

fn batch_value(
    result: &MusicInfoBatchResult,
    include_sub_index: bool,
) -> Result<String, RenderToolError> {
    let results = result
        .images
        .iter()
        .enumerate()
        .map(|(position, image)| image_value(image, position + 1, include_sub_index))
        .collect::<Vec<_>>();
    let mut payload = serde_json::Map::new();
    payload.insert("results".to_owned(), Value::Array(results.clone()));
    if !results.is_empty() {
        payload.insert("images".to_owned(), Value::Array(results));
    }
    if !result.errors.is_empty() {
        payload.insert(
            "errors".to_owned(),
            Value::Array(
                result
                    .errors
                    .iter()
                    .map(|error| error_value(error, !include_sub_index))
                    .collect(),
            ),
        );
    }
    serde_json::to_string(&Value::Object(payload)).map_err(|_| RenderToolError::Serialization)
}

fn image_path(image: &MusicInfoImage) -> Value {
    json!({
        "imagePath": image.image_path,
        "mimeType": "image/png",
        "width": image.width,
        "height": image.height,
    })
}

fn image_value(image: &MusicInfoImage, single_position: usize, include_sub_index: bool) -> Value {
    let mut value = image_path(image);
    if let Some(fields) = value.as_object_mut() {
        fields.insert(
            "index".to_owned(),
            json!(if include_sub_index {
                image.index
            } else {
                single_position
            }),
        );
        if include_sub_index {
            fields.insert("subIndex".to_owned(), json!(image.sub_index));
        }
        fields.insert("query".to_owned(), json!(image.query));
        fields.insert("musicId".to_owned(), json!(image.music_id));
        fields.insert("title".to_owned(), json!(image.title));
        fields.insert(
            "chartType".to_owned(),
            json!(image.chart_type.map_or("", |value| value.label())),
        );
    }
    value
}

fn error_value(error: &MusicInfoItemError, single: bool) -> Value {
    let mut value = serde_json::Map::new();
    let index = if single {
        error.sub_index.unwrap_or(error.index)
    } else {
        error.index
    };
    value.insert("index".to_owned(), json!(index.to_string()));
    value.insert("query".to_owned(), json!(error.query));
    if single && let Some(chart_type) = error.chart_type {
        value.insert("chartType".to_owned(), json!(chart_type.label()));
    }
    value.insert("message".to_owned(), json!(error.message));
    Value::Object(value)
}
