use maimai_app::{
    b50_image::B50ImageData,
    scores::{Lookup, PlayerScoreProfile},
};
use maimai_render::B50View;
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::{convert, dto::B50DataDto, error::B50ToolError, format};

pub(super) struct RenderData {
    pub view: B50View,
    pub filename_stem: String,
    pub lookup: Value,
    pub player: Value,
    pub counts: Value,
    pub rating_breakdown: Value,
    pub caption: String,
    pub nickname: Option<String>,
    pub rating: Option<u32>,
}

pub(super) fn provided(
    data: &B50DataDto,
    title: Option<String>,
) -> Result<RenderData, B50ToolError> {
    let player = data.player.as_ref();
    Ok(RenderData {
        view: convert::b50_view(data, title)?,
        filename_stem: convert::filename_stem(data),
        lookup: value(&data.lookup)?,
        player: value(&data.player)?,
        counts: value(&data.counts)?,
        rating_breakdown: value(&data.rating_breakdown)?,
        caption: format::caption(data),
        nickname: player.and_then(|value| value.nickname.clone()),
        rating: player.and_then(|value| value.rating),
    })
}

pub(super) fn queried(data: B50ImageData, now: OffsetDateTime) -> RenderData {
    let lookup = match &data.result.lookup {
        Lookup::Qq(qq) => json!({"qq":qq.as_str()}),
        Lookup::Username(username) => json!({"username":username.as_str()}),
    };
    let player = player(&data.result.player);
    let filename_stem = match &data.result.lookup {
        Lookup::Qq(qq) => qq.as_str().to_owned(),
        Lookup::Username(username) => username.as_str().to_owned(),
    };
    RenderData {
        view: data.view,
        filename_stem,
        lookup,
        player,
        counts: json!({
            "sd":data.result.b35.len(),
            "dx":data.result.b15.len(),
            "total":data.result.total_count(),
        }),
        rating_breakdown: json!({
            "sd":data.result.rating_breakdown.b35,
            "dx":data.result.rating_breakdown.b15,
            "total":data.result.rating_breakdown.total,
        }),
        caption: format::queried_caption(data.selection.source, data.result.source, now),
        nickname: data.result.player.nickname,
        rating: data
            .result
            .player
            .rating
            .or(data.result.player.actual_rating),
    }
}

fn player(value: &PlayerScoreProfile) -> Value {
    json!({
        "nickname":value.nickname,
        "username":value.username,
        "rating":value.rating,
        "actualRating":value.actual_rating,
        "additionalRating":value.additional_rating,
        "plate":value.plate,
    })
}

fn value(value: &impl serde::Serialize) -> Result<Value, B50ToolError> {
    serde_json::to_value(value).map_err(|_| B50ToolError::Internal)
}
