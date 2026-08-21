use maimai_app::rating_ranking::RatingRankingImage;
use serde_json::json;

use crate::render_tools::RenderToolError;

pub(super) fn image(image: &RatingRankingImage) -> Result<String, RenderToolError> {
    serde_json::to_string(&json!({
        "imagePath": image.image_path,
        "mimeType": "image/png",
        "width": image.width,
        "height": image.height,
    }))
    .map_err(|_| RenderToolError::Serialization)
}
