use maimai_app::score_list::ScoreListImage;
use serde_json::{Map, Value, json};

use super::{ScoreListSurface, error::ScoreListToolError};
use crate::render_tools::score_source;

pub(super) fn success(
    image: &ScoreListImage,
    surface: ScoreListSurface,
) -> Result<String, ScoreListToolError> {
    let mut payload = Map::new();
    payload.insert("imagePath".to_owned(), json!(image.image_path));
    payload.insert("mimeType".to_owned(), json!("image/png"));
    payload.insert("width".to_owned(), json!(image.width));
    payload.insert("height".to_owned(), json!(image.height));
    if surface == ScoreListSurface::Main {
        score_source::insert(&mut payload, image.source);
    }
    serde_json::to_string(&Value::Object(payload)).map_err(|_| ScoreListToolError::Serialization)
}
