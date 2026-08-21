use maimai_core::{ChartConstant, RatingError};
use serde_json::Number;

use crate::CatalogError;

pub(super) fn parse_chart_constant(
    source_name: &'static str,
    song_id: &str,
    number: &Number,
) -> Result<ChartConstant, CatalogError> {
    let value = number.to_string();
    ChartConstant::from_decimal_str(&value).map_err(|_source: RatingError| {
        CatalogError::InvalidNumber {
            source_name,
            song_id: song_id.to_owned(),
            field: "chartConstant",
            value,
        }
    })
}

pub(super) fn number_to_u32(number: &Number) -> Option<u32> {
    if let Some(value) = number.as_u64() {
        return u32::try_from(value).ok();
    }
    let value = number.to_string();
    let (integer, fraction) = value.split_once('.')?;
    if !fraction.bytes().all(|byte| byte == b'0') {
        return None;
    }
    integer.parse::<u32>().ok()
}
