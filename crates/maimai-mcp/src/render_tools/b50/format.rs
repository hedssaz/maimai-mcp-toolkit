use std::time::Duration;

use maimai_app::b50_render::RenderedB50;
use serde_json::{Map, Value, json};
use time::{OffsetDateTime, UtcOffset};

use super::{error::B50RenderToolError, handler::RenderDeployment};
use crate::render_tools::score_source;

pub(super) fn success(
    result: &RenderedB50,
    deployment: RenderDeployment,
) -> Result<String, B50RenderToolError> {
    let mut payload = Map::new();
    payload.insert("imagePath".to_owned(), json!(result.image_path));
    payload.insert("mimeType".to_owned(), json!("image/png"));
    payload.insert("width".to_owned(), json!(result.width));
    payload.insert("height".to_owned(), json!(result.height));
    if deployment == RenderDeployment::Main {
        payload.insert(
            "timings".to_owned(),
            json!({
                "setup_ms": 0.0,
                "query_ms": milliseconds(result.timings.query),
                "userinfo_ms": milliseconds(result.timings.prepare),
                "caption_ms": 0.0,
                "draw_ms": milliseconds(result.timings.draw),
                "save_ms": milliseconds(result.timings.save),
            }),
        );
        payload.insert("caption".to_owned(), json!(caption(result)));
    }
    serde_json::to_string(&Value::Object(payload)).map_err(|_| B50RenderToolError::Serialization)
}

fn caption(result: &RenderedB50) -> String {
    let mut lines = vec![score_source::caption(result.source)];
    if let Some(computed_at) = result.local_computed_at {
        lines.insert(0, format!("成绩更新时间：{}", china_timestamp(computed_at)));
    }
    lines.join("\n")
}

fn china_timestamp(value: OffsetDateTime) -> String {
    let china = UtcOffset::from_hms(8, 0, 0).unwrap_or(UtcOffset::UTC);
    let value = value.to_offset(china);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn milliseconds(value: Duration) -> f64 {
    value.as_secs_f64() * 1_000.0
}
