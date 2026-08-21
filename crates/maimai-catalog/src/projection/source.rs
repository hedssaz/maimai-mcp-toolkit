use std::collections::HashMap;

use crate::{
    CatalogError,
    enrich::release_date,
    metadata::{RegionAvailability, SourceKind, SourceSongFields, SourceSongProjection},
    raw::{ChartStatsDocument, DivingFishSong, DxSong, LxnsSong, OfficialSong},
};
use maimai_core::{ChartGeneration, SongIdNamespace, SourceSongId};

use super::{chart, source_id, stats::number_decimal};

pub(super) fn lxns(
    raw: &LxnsSong,
    versions: &HashMap<u32, &str>,
    stats: &ChartStatsDocument,
) -> Result<SourceSongProjection, CatalogError> {
    let id = SourceSongId::numeric(SongIdNamespace::Lxns, raw.id);
    let mut charts = Vec::new();
    chart::append_lxns(
        &mut charts,
        &raw.difficulties.standard,
        ChartGeneration::Standard,
        &id,
        stats,
    )?;
    chart::append_lxns(
        &mut charts,
        &raw.difficulties.dx,
        ChartGeneration::Deluxe,
        &id,
        stats,
    )?;
    chart::append_lxns(
        &mut charts,
        &raw.difficulties.utage,
        ChartGeneration::UtageOnePlayer,
        &id,
        stats,
    )?;
    Ok(SourceSongProjection {
        source: SourceKind::China,
        id,
        title: raw.title.clone(),
        artist: raw.artist.clone(),
        genre: raw.genre.clone(),
        bpm: Some(number_decimal(
            &raw.bpm,
            "LXNS",
            &raw.id.to_string(),
            "bpm",
        )?),
        version: versions
            .get(&raw.version)
            .map_or_else(|| raw.version.to_string(), |value| (*value).to_owned()),
        release_date: None,
        is_new: None,
        is_locked: None,
        image_name: None,
        source_fields: SourceSongFields {
            numeric_version: Some(raw.version),
            ..SourceSongFields::default()
        },
        charts,
    })
}

pub(super) fn diving_fish(
    raw: &DivingFishSong,
    id: SourceSongId,
    stats: &ChartStatsDocument,
) -> Result<SourceSongProjection, CatalogError> {
    let source_generation = match raw.chart_type.trim().to_ascii_uppercase().as_str() {
        "SD" => ChartGeneration::Standard,
        "DX" => ChartGeneration::Deluxe,
        value => {
            return Err(CatalogError::UnsupportedSourceValue {
                source_name: "Diving-Fish",
                song_id: raw.id.clone(),
                field: "type",
                value: value.to_owned(),
            });
        }
    };
    let generation = if matches!(id.value(), maimai_core::SongIdValue::Numeric(value) if *value >= 100_000)
    {
        ChartGeneration::UtageOnePlayer
    } else {
        source_generation
    };
    let charts = chart::diving_fish(raw, &id, generation, stats)?;
    Ok(SourceSongProjection {
        source: SourceKind::DivingFish,
        id,
        title: raw.title.clone(),
        artist: raw.basic_info.artist.clone(),
        genre: raw.basic_info.genre.clone(),
        bpm: Some(number_decimal(
            &raw.basic_info.bpm,
            "Diving-Fish",
            &raw.id,
            "bpm",
        )?),
        version: raw.basic_info.version.clone(),
        release_date: None,
        is_new: None,
        is_locked: None,
        image_name: None,
        source_fields: SourceSongFields::default(),
        charts,
    })
}

pub(super) fn dxdata(
    raw: &DxSong,
    stats: &ChartStatsDocument,
) -> Result<SourceSongProjection, CatalogError> {
    let id = source_id(SongIdNamespace::DxRating, &raw.song_id, "dxdata")?;
    let charts = raw
        .sheets
        .iter()
        .map(|sheet| chart::dxdata(sheet, &id, stats))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SourceSongProjection {
        source: SourceKind::Japan,
        id,
        title: raw.title.clone(),
        artist: raw.artist.clone(),
        genre: raw.category.clone(),
        bpm: raw
            .bpm
            .as_ref()
            .map(|value| number_decimal(value, "dxdata", &raw.song_id, "bpm"))
            .transpose()?,
        version: charts
            .iter()
            .find_map(|chart| (!chart.version.is_empty()).then_some(chart.version.clone()))
            .unwrap_or_default(),
        release_date: raw
            .sheets
            .iter()
            .filter_map(|sheet| sheet.release_date.as_deref())
            .map(|value| release_date(value, &raw.song_id))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .min(),
        is_new: Some(raw.is_new),
        is_locked: Some(raw.is_locked),
        image_name: raw.image_name.clone(),
        source_fields: SourceSongFields {
            category: Some(raw.category.clone()),
            search_acronyms: raw.search_acronyms.clone(),
            ..SourceSongFields::default()
        },
        charts,
    })
}

pub(super) fn official(
    raw: &OfficialSong,
    stats: &ChartStatsDocument,
) -> Result<SourceSongProjection, CatalogError> {
    let id = SourceSongId::numeric(SongIdNamespace::OfficialCn, raw.id);
    let charts = raw
        .sheets
        .iter()
        .map(|sheet| chart::official(sheet, &id, stats))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SourceSongProjection {
        source: SourceKind::Official,
        id,
        title: raw.title.clone(),
        artist: raw.artist.clone(),
        genre: if raw.genre.is_empty() {
            raw.category.clone()
        } else {
            raw.genre.clone()
        },
        bpm: raw
            .bpm
            .as_ref()
            .map(|value| number_decimal(value, "official", &raw.id.to_string(), "bpm"))
            .transpose()?,
        version: raw.version.clone(),
        release_date: None,
        is_new: None,
        is_locked: None,
        image_name: raw.cover_id.map(|id| id.to_string()),
        source_fields: SourceSongFields {
            numeric_version: None,
            release_version: raw.release_version,
            official_add_version: raw.official_add_version.clone(),
            category: Some(raw.category.clone()),
            asset_dir: raw.asset_dir.clone(),
            jacket_path: raw.jacket_path.clone(),
            rights: None,
            map: None,
            slug: None,
            keyword: None,
            comment: None,
            search_acronyms: Vec::new(),
        },
        charts,
    })
}

pub(super) fn raw_regions(value: crate::raw::DxRegions, source: SourceKind) -> RegionAvailability {
    RegionAvailability {
        jp: value.jp || source == SourceKind::Japan,
        intl: value.intl,
        usa: value.usa,
        cn: matches!(source, SourceKind::China | SourceKind::DivingFish),
    }
}
