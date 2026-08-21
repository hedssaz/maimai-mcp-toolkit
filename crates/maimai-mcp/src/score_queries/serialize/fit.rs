use std::str::FromStr;

use maimai_app::scores::{ExactRatio, FitIndex, FitIndexLabel, FitIndexSection};
use rust_decimal::{Decimal, RoundingStrategy};
use serde_json::{Number, Value, json};

use crate::score_queries::error::ScoreQueryToolError;

pub(super) fn fit_index(value: FitIndex) -> Result<Value, ScoreQueryToolError> {
    Ok(
        json!({"available":value.available(),"label":value.label.map(label),
        "b50":section(value.b50)?,"b35":section(value.b35)?,"b15":section(value.b15)?}),
    )
}

fn section(value: FitIndexSection) -> Result<Value, ScoreQueryToolError> {
    Ok(json!({
        "virtualRating":value.virtual_rating.map(|value| number(format!("{value}.0"))).transpose()?,
        "virtualRatio":value.virtual_ratio_percent.map(ratio_number).transpose()?,
        "weightedAvgFitDelta":value.weighted_average_delta.map(ratio_number).transpose()?,
        "counted":value.counted,"missing":value.missing,
        "totalRa":value.total_rating.map(|value| number(format!("{value}.0"))).transpose()?,
    }))
}

fn ratio_number(value: ExactRatio) -> Result<Number, ScoreQueryToolError> {
    let numerator = Decimal::try_from_i128_with_scale(value.numerator(), 0)
        .map_err(|_| ScoreQueryToolError::internal())?;
    let denominator =
        i128::try_from(value.denominator()).map_err(|_| ScoreQueryToolError::internal())?;
    let denominator = Decimal::try_from_i128_with_scale(denominator, 0)
        .map_err(|_| ScoreQueryToolError::internal())?;
    let ratio = numerator
        .checked_div(denominator)
        .ok_or_else(ScoreQueryToolError::internal)?
        .round_dp_with_strategy(15, RoundingStrategy::MidpointNearestEven)
        .normalize();
    let mut text = ratio.to_string();
    if !text.contains('.') {
        text.push_str(".0");
    }
    number(text)
}

fn number(value: String) -> Result<Number, ScoreQueryToolError> {
    Number::from_str(&value).map_err(|_| ScoreQueryToolError::internal())
}

const fn label(value: FitIndexLabel) -> &'static str {
    match value {
        FitIndexLabel::ClearlyInflated => "明显虚高（水）",
        FitIndexLabel::SlightlyInflated => "略微虚高",
        FitIndexLabel::Balanced => "基本持平",
        FitIndexLabel::SlightlyDeflated => "略微虚低",
        FitIndexLabel::ClearlyDeflated => "明显虚低（硬实力）",
    }
}
