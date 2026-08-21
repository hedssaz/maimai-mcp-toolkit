use maimai_app::rise_score::RiseScoreImage;
use serde_json::{Map, Value, json};

use super::{RiseScoreSurface, error::RiseScoreToolError};
use crate::render_tools::score_source;

pub(super) fn success(
    image: &RiseScoreImage,
    surface: RiseScoreSurface,
) -> Result<String, RiseScoreToolError> {
    let mut payload = Map::new();
    payload.insert("imagePath".to_owned(), json!(image.image_path));
    payload.insert("mimeType".to_owned(), json!("image/png"));
    payload.insert("width".to_owned(), json!(image.width));
    payload.insert("height".to_owned(), json!(image.height));
    if surface == RiseScoreSurface::Main {
        score_source::insert(&mut payload, image.source);
    }
    serde_json::to_string(&Value::Object(payload)).map_err(|_| RiseScoreToolError::Serialization)
}
