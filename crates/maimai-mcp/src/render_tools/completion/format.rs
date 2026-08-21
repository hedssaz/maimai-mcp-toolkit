use maimai_app::completion::{
    CompletionImage, CompletionResponse, PlateBatchResult, PlateProgressBatchResult,
    PlateProgressOutput, RatingTableImage,
};
use maimai_catalog::PlateServer;
use maimai_core::ScoreSource;
use serde_json::{Map, Value, json};

use super::{CompletionSurface, CompletionToolError};
use crate::render_tools::score_source;

pub(super) fn image(
    result: &CompletionResponse<CompletionImage>,
    surface: CompletionSurface,
) -> Result<String, CompletionToolError> {
    let mut value = image_path(&result.value);
    apply_source(&mut value, result.source, surface);
    stringify(value)
}

pub(super) fn rating_image(
    result: &CompletionResponse<RatingTableImage>,
    surface: CompletionSurface,
) -> Result<String, CompletionToolError> {
    let mut value = json!({
        "imagePath":result.value.path,
        "mimeType":"image/png",
        "width":result.value.width,
        "height":result.value.height,
    });
    apply_source(&mut value, result.source, surface);
    stringify(value)
}

pub(super) fn plate_batch(
    result: &PlateBatchResult,
    surface: CompletionSurface,
) -> Result<String, CompletionToolError> {
    let results = result
        .results
        .iter()
        .map(|image| {
            let mut value = plate_image(image);
            if let Some(source) = result.source {
                apply_source(&mut value, source, surface);
            }
            value
        })
        .collect::<Vec<_>>();
    let mut payload = Map::new();
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
                    .map(|error| {
                        json!({
                            "index":error.index.to_string(),
                            "label":error.label,
                            "message":error.message,
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(source) = result.source {
        apply_source_object(&mut payload, source, surface);
    }
    stringify(Value::Object(payload))
}

pub(super) fn plate_progress(
    result: &CompletionResponse<PlateProgressOutput>,
    surface: CompletionSurface,
) -> Result<String, CompletionToolError> {
    match &result.value {
        PlateProgressOutput::Text(text) => Ok(append_caption(text, result.source, surface)),
        PlateProgressOutput::Image(image) => {
            let mut value = image_path(image);
            apply_source(&mut value, result.source, surface);
            stringify(value)
        }
    }
}

pub(super) fn plate_progress_batch(
    result: &PlateProgressBatchResult,
    surface: CompletionSurface,
) -> Result<String, CompletionToolError> {
    let values = result
        .results
        .iter()
        .map(|item| {
            let mut value = match &item.output {
                PlateProgressOutput::Text(text) => json!({"text":text}),
                PlateProgressOutput::Image(image) => image_path(image),
            };
            if let Some(fields) = value.as_object_mut() {
                fields.insert("index".to_owned(), json!(item.index));
                fields.insert("version".to_owned(), json!(item.version));
                fields.insert("plan".to_owned(), json!(item.target));
                fields.insert("server".to_owned(), json!(server(item.server)));
                fields.insert(
                    "label".to_owned(),
                    json!(format!("{}{}", item.version, item.target)),
                );
            }
            if matches!(&item.output, PlateProgressOutput::Image(_))
                && let Some(source) = result.source
            {
                apply_source(&mut value, source, surface);
            }
            value
        })
        .collect::<Vec<_>>();
    let images = values
        .iter()
        .filter(|value| value.get("imagePath").is_some())
        .cloned()
        .collect::<Vec<_>>();
    let mut payload = Map::new();
    payload.insert("results".to_owned(), Value::Array(values));
    if !images.is_empty() {
        payload.insert("images".to_owned(), Value::Array(images));
    }
    if !result.errors.is_empty() {
        payload.insert(
            "errors".to_owned(),
            Value::Array(
                result
                    .errors
                    .iter()
                    .map(|error| {
                        json!({
                            "index":error.index.to_string(),
                            "label":error.label,
                            "message":error.message,
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(source) = result.source {
        apply_source_object(&mut payload, source, surface);
    }
    stringify(Value::Object(payload))
}

fn image_path(image: &CompletionImage) -> Value {
    json!({
        "imagePath":image.path,
        "mimeType":"image/png",
        "width":image.width,
        "height":image.height,
    })
}

fn plate_image(image: &CompletionImage) -> Value {
    let mut value = image_path(image);
    if let Some(fields) = value.as_object_mut() {
        fields.insert("index".to_owned(), json!(image.index));
        fields.insert("version".to_owned(), json!(image.version));
        fields.insert("plan".to_owned(), json!(image.target));
        fields.insert("server".to_owned(), json!(server(image.server)));
        fields.insert(
            "label".to_owned(),
            json!(format!("{}{}", image.version, image.target)),
        );
    }
    value
}

fn apply_source(value: &mut Value, source: ScoreSource, surface: CompletionSurface) {
    if let Some(fields) = value.as_object_mut() {
        apply_source_object(fields, source, surface);
    }
}

fn apply_source_object(
    fields: &mut Map<String, Value>,
    source: ScoreSource,
    surface: CompletionSurface,
) {
    if surface == CompletionSurface::Public {
        return;
    }
    score_source::insert(fields, source);
}

fn append_caption(text: &str, source: ScoreSource, surface: CompletionSurface) -> String {
    if surface == CompletionSurface::Public {
        text.to_owned()
    } else {
        format!("{}\n\n{}", text.trim_end(), score_source::caption(source))
    }
}

fn server(value: PlateServer) -> &'static str {
    match value {
        PlateServer::Cn => "cn",
        PlateServer::Jp => "jp",
        PlateServer::Custom => "custom",
    }
}

fn stringify(value: Value) -> Result<String, CompletionToolError> {
    serde_json::to_string(&value).map_err(|_| CompletionToolError::Serialization)
}

#[cfg(test)]
mod tests {
    use super::append_caption;
    use crate::render_tools::completion::CompletionSurface;
    use maimai_core::ScoreSource;

    #[test]
    fn public_text_has_no_main_source_caption() {
        assert_eq!(
            append_caption("正文", ScoreSource::DivingFish, CompletionSurface::Public),
            "正文"
        );
    }
}
