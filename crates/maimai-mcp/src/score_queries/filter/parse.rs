use maimai_app::scores::FitLabel;
use rust_decimal::Decimal;
use serde_json::Number;

use super::{Section, SortKey, SortOrder};
use crate::score_queries::error::ScoreQueryToolError;

pub(super) fn section(value: Option<&str>) -> Result<Section, ScoreQueryToolError> {
    match value.unwrap_or("b50") {
        "b50" | "all" => Ok(Section::B50),
        "b35" | "sd" | "old" => Ok(Section::B35),
        "b15" | "dx" | "new" => Ok(Section::B15),
        "split" | "both" => Ok(Section::Split),
        _ => Err(ScoreQueryToolError::invalid("section 格式不正确。")),
    }
}

pub(super) fn sort_key(value: Option<&str>) -> Result<SortKey, ScoreQueryToolError> {
    match value.unwrap_or("default") {
        "default" => Ok(SortKey::Default),
        "ra" => Ok(SortKey::Rating),
        "achievement" => Ok(SortKey::Achievement),
        "ds" => Ok(SortKey::Constant),
        "fitDiff" => Ok(SortKey::FitConstant),
        "fitDelta" => Ok(SortKey::FitDelta),
        "title" => Ok(SortKey::Title),
        _ => Err(ScoreQueryToolError::invalid("sortBy 格式不正确。")),
    }
}

pub(super) fn sort_order(value: Option<&str>) -> Result<SortOrder, ScoreQueryToolError> {
    match value.unwrap_or("desc") {
        "asc" => Ok(SortOrder::Ascending),
        "desc" => Ok(SortOrder::Descending),
        _ => Err(ScoreQueryToolError::invalid("sortOrder 格式不正确。")),
    }
}

pub(super) fn fit_label(value: Option<&str>) -> Result<Option<FitLabel>, ScoreQueryToolError> {
    value
        .map(|value| match value.trim() {
            "虚高" | "高" | "over" | "overrated" => Ok(FitLabel::Inflated),
            "虚低" | "低" | "under" | "underrated" => Ok(FitLabel::Deflated),
            _ => Err(ScoreQueryToolError::invalid(
                "fitLabel 必须是 虚高 或 虚低。",
            )),
        })
        .transpose()
}

pub(super) fn decimal<T, E>(
    value: Option<&Number>,
    field: &'static str,
    parse: impl FnOnce(&str) -> Result<T, E> + Copy,
) -> Result<Option<T>, ScoreQueryToolError> {
    value
        .map(|value| {
            parse(&value.to_string())
                .map_err(|_| ScoreQueryToolError::invalid(format!("{field} 格式不正确。")))
        })
        .transpose()
}

pub(super) fn decimal_value(
    value: Option<&Number>,
    field: &'static str,
) -> Result<Option<Decimal>, ScoreQueryToolError> {
    value
        .map(|value| {
            value
                .to_string()
                .parse()
                .map_err(|_| ScoreQueryToolError::invalid(format!("{field} 格式不正确。")))
        })
        .transpose()
}

pub(super) fn integer(
    value: Option<&Number>,
    field: &'static str,
) -> Result<Option<u32>, ScoreQueryToolError> {
    value
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| ScoreQueryToolError::invalid(format!("{field} 必须是非负整数。")))
        })
        .transpose()
}
