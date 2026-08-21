use maimai_core::{ChartGeneration, Difficulty, NoteCounts, SongIdValue, SourceSongId};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde_json::Number;
use time::{Date, Month};

use crate::{
    CatalogError,
    metadata::RegionAvailability,
    raw::{DxNoteCounts, DxRegions, DxSheet},
};

pub(crate) fn generation(value: &str, song_id: &str) -> Result<ChartGeneration, CatalogError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "std" => Ok(ChartGeneration::Standard),
        "dx" => Ok(ChartGeneration::Deluxe),
        "utage2p" => Ok(ChartGeneration::UtageTwoPlayer),
        "utage" | "utage1p" => Ok(ChartGeneration::UtageOnePlayer),
        _ => Err(unsupported(song_id, "type", value)),
    }
}

pub(crate) fn difficulty_for_generation(
    generation: ChartGeneration,
    value: &str,
    song_id: &str,
) -> Result<Difficulty, CatalogError> {
    if matches!(
        generation,
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
    ) {
        return Ok(Difficulty::Utage);
    }
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace([':', '_', ' '], "");
    match normalized.as_str() {
        "basic" => Ok(Difficulty::Basic),
        "advanced" => Ok(Difficulty::Advanced),
        "expert" => Ok(Difficulty::Expert),
        "master" => Ok(Difficulty::Master),
        "remaster" => Ok(Difficulty::ReMaster),
        _ => Err(unsupported(song_id, "difficulty", value)),
    }
}

pub(crate) fn chart_identity(
    sheet: &DxSheet,
    song_id: &str,
) -> Result<(ChartGeneration, Difficulty), CatalogError> {
    let generation = generation(&sheet.chart_type, song_id)?;
    let difficulty = difficulty_for_generation(generation, &sheet.difficulty, song_id)?;
    Ok((generation, difficulty))
}

pub(crate) const fn difficulty_index(value: Difficulty) -> usize {
    match value {
        Difficulty::Basic => 0,
        Difficulty::Advanced => 1,
        Difficulty::Expert => 2,
        Difficulty::Master => 3,
        Difficulty::ReMaster => 4,
        Difficulty::Utage => 0,
    }
}

pub(crate) fn note_counts(value: &DxNoteCounts) -> NoteCounts {
    NoteCounts {
        tap: value.tap.unwrap_or_default(),
        hold: value.hold.unwrap_or_default(),
        slide: value.slide.unwrap_or_default(),
        touch: value.touch.unwrap_or_default(),
        break_notes: value.break_notes.unwrap_or_default(),
    }
}

pub(crate) fn regions(value: DxRegions) -> RegionAvailability {
    RegionAvailability {
        jp: value.jp,
        intl: value.intl,
        usa: value.usa,
        cn: value.cn,
    }
}

pub(crate) fn number_u32(
    value: &Number,
    source_name: &'static str,
    song_id: &str,
    field: &'static str,
) -> Result<u32, CatalogError> {
    let decimal = number_decimal(value, source_name, song_id, field)?;
    if !decimal.fract().is_zero() {
        return Err(CatalogError::InvalidNumber {
            source_name,
            song_id: song_id.to_owned(),
            field,
            value: value.to_string(),
        });
    }
    decimal.to_u32().ok_or_else(|| CatalogError::InvalidNumber {
        source_name,
        song_id: song_id.to_owned(),
        field,
        value: value.to_string(),
    })
}

pub(crate) fn release_date(value: &str, song_id: &str) -> Result<Date, CatalogError> {
    let mut parts = value.split('-');
    let year = parts.next().and_then(|value| value.parse::<i32>().ok());
    let month = parts
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .and_then(|value| Month::try_from(value).ok());
    let day = parts.next().and_then(|value| value.parse::<u8>().ok());
    if parts.next().is_some() {
        return Err(invalid_release_date(song_id, value));
    }
    match (year, month, day) {
        (Some(year), Some(month), Some(day)) => Date::from_calendar_date(year, month, day)
            .map_err(|_| invalid_release_date(song_id, value)),
        _ => Err(invalid_release_date(song_id, value)),
    }
}

pub(crate) fn source_id_label(source_id: &SourceSongId) -> String {
    match source_id.value() {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}

pub(crate) fn number_decimal(
    value: &Number,
    source_name: &'static str,
    song_id: &str,
    field: &'static str,
) -> Result<Decimal, CatalogError> {
    value
        .to_string()
        .parse::<Decimal>()
        .map_err(|_| CatalogError::InvalidNumber {
            source_name,
            song_id: song_id.to_owned(),
            field,
            value: value.to_string(),
        })
}

pub(crate) fn push_unique<T: Eq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn unsupported(song_id: &str, field: &'static str, value: &str) -> CatalogError {
    CatalogError::UnsupportedDxDataValue {
        song_id: song_id.to_owned(),
        field,
        value: value.to_owned(),
    }
}

fn invalid_release_date(song_id: &str, value: &str) -> CatalogError {
    CatalogError::InvalidReleaseDate {
        song_id: song_id.to_owned(),
        value: value.to_owned(),
    }
}
