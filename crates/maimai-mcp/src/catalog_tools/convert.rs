use std::{collections::BTreeSet, str::FromStr};

use maimai_catalog::{
    CatalogQuery, CatalogSnapshot, CatalogSort, FitLabel, InclusiveRange, NewSongSource, Region,
    SongIdFilter, SortDirection, SortKey,
};
use maimai_core::{ChartConstant, ChartGeneration, Difficulty, SongIdValue};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use time::{Date, Month};

use super::{
    dto::{OneOrManyScalar, OneOrManyString, Scalar, SearchArgs},
    error::CatalogToolError,
};

pub(super) fn catalog_query(
    snapshot: &CatalogSnapshot,
    arguments: &SearchArgs,
) -> Result<CatalogQuery, CatalogToolError> {
    let constant = constant_range(
        arguments.ds.as_ref(),
        arguments.ds_min.as_ref(),
        arguments.ds_max.as_ref(),
    )?;
    let (fit_diff, fit_diff_max_exclusive) = fit_range(
        arguments.fit_diff.as_ref(),
        arguments.fit_diff_min.as_ref(),
        arguments.fit_diff_max.as_ref(),
    )?;
    let (fit_delta, _) = decimal_range(
        arguments.fit_delta.as_ref(),
        arguments.fit_delta_min.as_ref(),
        arguments.fit_delta_max.as_ref(),
        false,
        "fit_delta",
    )?;
    let (id, id_range) = id_filter(arguments.id.as_ref(), arguments.id_min, arguments.id_max)?;
    Ok(CatalogQuery {
        query: arguments.query.clone(),
        id,
        id_range,
        level: arguments.level.clone(),
        genre: arguments.genre.clone(),
        version: arguments.version.clone(),
        constant,
        fit_diff,
        fit_diff_max_exclusive,
        fit_delta,
        fit_label: arguments.fit_label.as_deref().map(fit_label).transpose()?,
        difficulties: arguments
            .difficulty
            .as_deref()
            .map(difficulties)
            .transpose()?
            .unwrap_or_default(),
        generations: arguments
            .song_type
            .as_deref()
            .map(generations)
            .transpose()?
            .unwrap_or_default(),
        bpm: bpm_range(
            arguments.bpm.as_ref(),
            arguments.bpm_min.as_ref(),
            arguments.bpm_max.as_ref(),
        )?,
        artist: arguments.artist.clone(),
        charter: arguments.charter.clone(),
        region_has: regions(arguments.region_has.as_ref())?,
        region_missing: regions(arguments.region_missing.as_ref())?,
        is_new: arguments.is_new,
        is_new_source: new_source(arguments.is_new_source.as_deref())?,
        is_locked: arguments.is_locked,
        required_tag_ids: tags(snapshot, arguments.tag.as_ref())?,
        excluded_tag_ids: tags(snapshot, arguments.tag_exclude.as_ref())?,
        released_after: arguments
            .released_after
            .as_deref()
            .map(|value| date_bound(value, false))
            .transpose()?,
        released_before: arguments
            .released_before
            .as_deref()
            .map(|value| date_bound(value, true))
            .transpose()?,
        sort: arguments
            .sort
            .as_deref()
            .map(sort)
            .transpose()?
            .unwrap_or_default(),
        limit: arguments.limit,
    })
}

fn constant_range(
    value: Option<&Scalar>,
    min: Option<&serde_json::Number>,
    max: Option<&serde_json::Number>,
) -> Result<InclusiveRange<ChartConstant>, CatalogToolError> {
    let (mut low, mut high) = scalar_decimal_range(value, "ds")?;
    if let Some(value) = min {
        low = Some(decimal(&value.to_string(), "ds_min")?);
    }
    if let Some(value) = max {
        high = Some(decimal(&value.to_string(), "ds_max")?);
    }
    Ok(InclusiveRange::new(
        low.map(chart_constant).transpose()?,
        high.map(chart_constant).transpose()?,
    ))
}

fn fit_range(
    value: Option<&Scalar>,
    min: Option<&serde_json::Number>,
    max: Option<&serde_json::Number>,
) -> Result<(InclusiveRange<Decimal>, bool), CatalogToolError> {
    decimal_range(value, min, max, true, "fit_diff")
}

fn decimal_range(
    value: Option<&Scalar>,
    min: Option<&serde_json::Number>,
    max: Option<&serde_json::Number>,
    bucket_single: bool,
    field: &'static str,
) -> Result<(InclusiveRange<Decimal>, bool), CatalogToolError> {
    let (mut low, mut high) = scalar_decimal_range(value, field)?;
    let explicit_range = low.is_some() && high.is_some() && low != high;
    if let Some(value) = min {
        low = Some(decimal(&value.to_string(), field)?);
    }
    if let Some(value) = max {
        high = Some(decimal(&value.to_string(), field)?);
    }
    let has_explicit_bounds = min.is_some() || max.is_some() || explicit_range;
    if bucket_single && !has_explicit_bounds && low.is_some() && low == high {
        high = low.and_then(|value| value.checked_add(Decimal::new(1, 1)));
        return Ok((InclusiveRange::new(low, high), true));
    }
    Ok((InclusiveRange::new(low, high), false))
}

fn scalar_decimal_range(
    value: Option<&Scalar>,
    field: &'static str,
) -> Result<(Option<Decimal>, Option<Decimal>), CatalogToolError> {
    let Some(value) = value else {
        return Ok((None, None));
    };
    let text = value.text();
    if let Some((low, high)) = split_range(&text) {
        return Ok((Some(decimal(low, field)?), Some(decimal(high, field)?)));
    }
    let value = decimal(&text, field)?;
    Ok((Some(value), Some(value)))
}

fn split_range(value: &str) -> Option<(&str, &str)> {
    value.char_indices().skip(1).find_map(|(index, character)| {
        if character != '-' {
            return None;
        }
        let (left, right_with_separator) = value.split_at(index);
        let right = &right_with_separator[1..];
        (left.parse::<Decimal>().is_ok() && right.parse::<Decimal>().is_ok())
            .then_some((left, right))
    })
}

fn bpm_range(
    value: Option<&Scalar>,
    min: Option<&serde_json::Number>,
    max: Option<&serde_json::Number>,
) -> Result<InclusiveRange<u32>, CatalogToolError> {
    let (mut low, mut high) = scalar_decimal_range(value, "bpm")?;
    if let Some(value) = min {
        low = Some(decimal(&value.to_string(), "bpm_min")?);
    }
    if let Some(value) = max {
        high = Some(decimal(&value.to_string(), "bpm_max")?);
    }
    Ok(InclusiveRange::new(
        low.and_then(|value| value.ceil().to_u32()),
        high.and_then(|value| value.floor().to_u32()),
    ))
}

fn id_filter(
    value: Option<&Scalar>,
    min: Option<u32>,
    max: Option<u32>,
) -> Result<(Option<SongIdFilter>, InclusiveRange<u32>), CatalogToolError> {
    let mut exact = None;
    let mut range = InclusiveRange::new(min, max);
    if let Some(value) = value {
        let text = value.text();
        if let Some((low, high)) = split_range(&text) {
            range.min = Some(parse_u32(low, "id")?);
            range.max = Some(parse_u32(high, "id")?);
        } else if let Ok(number) = text.parse::<u32>() {
            exact = Some(SongIdFilter::AnySource(SongIdValue::Numeric(number)));
        } else {
            exact = Some(SongIdFilter::AnySource(
                SongIdValue::text(text)
                    .map_err(|error| CatalogToolError::input(error.to_string()))?,
            ));
        }
    }
    if min.is_some() {
        range.min = min;
    }
    if max.is_some() {
        range.max = max;
    }
    Ok((exact, range))
}

fn difficulties(value: &str) -> Result<BTreeSet<Difficulty>, CatalogToolError> {
    let normalized = clean(value).replace([':', '_'], "");
    let difficulty = match normalized.as_str() {
        "basic" | "绿" | "綠" => Difficulty::Basic,
        "advanced" | "黄" | "黃" => Difficulty::Advanced,
        "expert" | "红" | "紅" => Difficulty::Expert,
        "master" | "紫" => Difficulty::Master,
        "remaster" | "白" => Difficulty::ReMaster,
        "utage" | "宴" => Difficulty::Utage,
        _ => return Err(CatalogToolError::input("unknown difficulty")),
    };
    Ok(BTreeSet::from([difficulty]))
}

fn generations(value: &str) -> Result<BTreeSet<ChartGeneration>, CatalogToolError> {
    let values = match clean(value).as_str() {
        "standard" | "sd" | "st" | "std" | "标准" | "標準" | "标" => {
            vec![ChartGeneration::Standard]
        }
        "dx" => vec![ChartGeneration::Deluxe],
        "utage" | "宴" | "宴会场" | "宴會場" => vec![
            ChartGeneration::UtageOnePlayer,
            ChartGeneration::UtageTwoPlayer,
        ],
        "utage1p" | "1p" | "单人宴" | "單人宴" | "单人" | "單人" => {
            vec![ChartGeneration::UtageOnePlayer]
        }
        "utage2p" | "2p" | "双人宴" | "雙人宴" | "合奏宴" | "合奏" => {
            vec![ChartGeneration::UtageTwoPlayer]
        }
        _ => return Err(CatalogToolError::input("unknown song_type")),
    };
    Ok(values.into_iter().collect())
}

fn regions(value: Option<&OneOrManyString>) -> Result<BTreeSet<Region>, CatalogToolError> {
    value.map_or_else(
        || Ok(BTreeSet::new()),
        |value| value.values().iter().map(|value| region(value)).collect(),
    )
}

fn region(value: &str) -> Result<Region, CatalogToolError> {
    match clean(value).as_str() {
        "jp" | "日服" => Ok(Region::Japan),
        "intl" | "国际服" | "國際服" => Ok(Region::International),
        "usa" | "美服" => Ok(Region::UnitedStates),
        "cn" | "国服" | "國服" => Ok(Region::China),
        _ => Err(CatalogToolError::input("unknown region")),
    }
}

fn tags(
    snapshot: &CatalogSnapshot,
    value: Option<&OneOrManyScalar>,
) -> Result<BTreeSet<u32>, CatalogToolError> {
    let Some(value) = value else {
        return Ok(BTreeSet::new());
    };
    let mut result = BTreeSet::new();
    for value in value.values() {
        for token in value.text().split([',', '/', '、', '，', ';', '；']) {
            let token = token.trim();
            if !token.is_empty() {
                result.insert(snapshot.resolve_tag(token)?);
            }
        }
    }
    Ok(result)
}

fn fit_label(value: &str) -> Result<FitLabel, CatalogToolError> {
    match clean(value).as_str() {
        "虚高" => Ok(FitLabel::Inflated),
        "虚低" => Ok(FitLabel::Deflated),
        _ => Err(CatalogToolError::input("fit_label must be 虚高 or 虚低")),
    }
}

fn sort(value: &str) -> Result<CatalogSort, CatalogToolError> {
    let (key, direction) = match clean(value).as_str() {
        "fit_delta_desc" | "虚高" | "最虚高" => (SortKey::FitDelta, SortDirection::Descending),
        "fit_delta_asc" | "虚低" | "最虚低" => (SortKey::FitDelta, SortDirection::Ascending),
        "fit_diff_asc" => (SortKey::FitDifference, SortDirection::Ascending),
        "fit_diff_desc" => (SortKey::FitDifference, SortDirection::Descending),
        _ => return Err(CatalogToolError::input("unknown sort")),
    };
    Ok(CatalogSort { key, direction })
}

fn new_source(value: Option<&str>) -> Result<NewSongSource, CatalogToolError> {
    match value.map(clean).as_deref() {
        None | Some("") => Ok(NewSongSource::Any),
        Some("cn") => Ok(NewSongSource::China),
        Some("jp") => Ok(NewSongSource::Japan),
        _ => Err(CatalogToolError::input("is_new_source must be cn or jp")),
    }
}

fn date_bound(value: &str, upper: bool) -> Result<Date, CatalogToolError> {
    let parts = value.trim().split('-').collect::<Vec<_>>();
    match parts.as_slice() {
        [year] if year.len() == 4 => {
            let year = parse_i32(year, "release date")?;
            let (month, day) = if upper {
                (Month::December, 31)
            } else {
                (Month::January, 1)
            };
            Date::from_calendar_date(year, month, day)
                .map_err(|error| CatalogToolError::input(error.to_string()))
        }
        [year, month] if year.len() == 4 && month.len() == 2 => {
            let year = parse_i32(year, "release date")?;
            let month = Month::try_from(parse_u8(month, "release date")?)
                .map_err(|error| CatalogToolError::input(error.to_string()))?;
            let day = if upper { month.length(year) } else { 1 };
            Date::from_calendar_date(year, month, day)
                .map_err(|error| CatalogToolError::input(error.to_string()))
        }
        [year, month, day] if year.len() == 4 && month.len() == 2 && day.len() == 2 => {
            let year = parse_i32(year, "release date")?;
            let month = Month::try_from(parse_u8(month, "release date")?)
                .map_err(|error| CatalogToolError::input(error.to_string()))?;
            Date::from_calendar_date(year, month, parse_u8(day, "release date")?)
                .map_err(|error| CatalogToolError::input(error.to_string()))
        }
        _ => Err(CatalogToolError::input(
            "release date must be YYYY, YYYY-MM, or YYYY-MM-DD",
        )),
    }
}

fn chart_constant(value: Decimal) -> Result<ChartConstant, CatalogToolError> {
    ChartConstant::from_decimal_str(&value.to_string())
        .map_err(|_| CatalogToolError::input("chart constant is out of range"))
}

fn decimal(value: &str, field: &str) -> Result<Decimal, CatalogToolError> {
    Decimal::from_str(value)
        .map_err(|_| CatalogToolError::input(format!("{field} must be a decimal number")))
}

fn parse_u32(value: &str, field: &str) -> Result<u32, CatalogToolError> {
    value
        .parse::<u32>()
        .map_err(|_| CatalogToolError::input(format!("{field} must be a non-negative integer")))
}

fn parse_i32(value: &str, field: &str) -> Result<i32, CatalogToolError> {
    value
        .parse::<i32>()
        .map_err(|_| CatalogToolError::input(format!("{field} must be an integer")))
}

fn parse_u8(value: &str, field: &str) -> Result<u8, CatalogToolError> {
    value
        .parse::<u8>()
        .map_err(|_| CatalogToolError::input(format!("{field} must be an integer")))
}

fn clean(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(' ', "")
}
