use std::collections::BTreeMap;

use maimai_core::{ChartGeneration, Difficulty, NoteCounts, SongIdValue, SourceSongId};
use rust_decimal::Decimal;
use serde_json::Number;

use crate::{
    CatalogError,
    enrich::{chart_identity, release_date},
    metadata::{RegionOverride, SourceChartProjection, SourceKind},
    raw::{
        ChartStatsDocument, DivingFishSong, DxNoteCounts, DxRegionOverride, DxSheet, LxnsChart,
        OfficialSheet, RawNoteCounts,
    },
};

use super::{
    source::raw_regions,
    stats::{fit_stats, number_decimal, stat_for},
};

pub(super) fn append_lxns(
    output: &mut Vec<SourceChartProjection>,
    charts: &[LxnsChart],
    default_generation: ChartGeneration,
    song_id: &SourceSongId,
    stats: &ChartStatsDocument,
) -> Result<(), CatalogError> {
    let numeric_id = numeric_source_id(song_id)?;
    for raw in charts {
        let generation = if default_generation == ChartGeneration::UtageOnePlayer && raw.is_buddy {
            ChartGeneration::UtageTwoPlayer
        } else {
            default_generation
        };
        let difficulty = if matches!(
            generation,
            ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
        ) {
            Difficulty::Utage
        } else {
            normal_difficulty(usize::from(raw.difficulty)).ok_or(
                CatalogError::InvalidDifficulty {
                    source_name: "LXNS",
                    song_id: numeric_id.to_string(),
                    difficulty: usize::from(raw.difficulty),
                },
            )?
        };
        let stat_id = fit_id(numeric_id, generation);
        let stat = stat_id.and_then(|id| {
            stat_for(stats, id, difficulty_index(difficulty), &raw.level).map(|value| (id, value))
        });
        output.push(SourceChartProjection {
            source_song_id: song_id.clone(),
            generation,
            difficulty,
            level: raw.level.clone(),
            constant: (!matches!(difficulty, Difficulty::Utage))
                .then(|| {
                    number_decimal(
                        &raw.level_value,
                        "LXNS",
                        &numeric_id.to_string(),
                        "level_value",
                    )
                })
                .transpose()?,
            note_designer: raw.note_designer.clone(),
            notes: Some(lxns_notes(raw.notes)),
            note_total: u32::try_from(lxns_notes(raw.notes).total()).ok(),
            version: raw
                .version
                .map_or_else(String::new, |value| value.to_string()),
            regions: raw_regions(Default::default(), SourceKind::China),
            release_date: None,
            internal_id: None,
            fit_source_id: stat.map(|(id, _)| id.to_string()),
            fit_stats: stat
                .map(|(_, value)| fit_stats(value, "LXNS", &numeric_id.to_string()))
                .transpose()?,
            music_id: Some(numeric_id),
            chart_id: stat_id,
            is_buddy: Some(raw.is_buddy),
            kanji: raw.kanji.clone(),
            description: raw.description.clone(),
            raw_difficulty: None,
            is_special: false,
            region_overrides: BTreeMap::new(),
            multiver_constants: BTreeMap::new(),
        });
    }
    Ok(())
}

pub(super) fn diving_fish(
    raw: &DivingFishSong,
    song_id: &SourceSongId,
    generation: ChartGeneration,
    stats: &ChartStatsDocument,
) -> Result<Vec<SourceChartProjection>, CatalogError> {
    let numeric_id = match song_id.value() {
        SongIdValue::Numeric(value) => Some(*value),
        SongIdValue::Text(_) => None,
    };
    raw.charts
        .iter()
        .enumerate()
        .map(|(index, chart)| {
            let difficulty = if matches!(
                generation,
                ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
            ) {
                Difficulty::Utage
            } else {
                normal_difficulty(index).ok_or(CatalogError::InvalidDifficulty {
                    source_name: "Diving-Fish",
                    song_id: raw.id.clone(),
                    difficulty: index,
                })?
            };
            let stat = if matches!(difficulty, Difficulty::Utage) {
                None
            } else {
                numeric_id.and_then(|id| {
                    stat_for(
                        stats,
                        id,
                        index,
                        raw.level.get(index).map_or("", String::as_str),
                    )
                    .map(|value| (id, value))
                })
            };
            Ok(SourceChartProjection {
                source_song_id: song_id.clone(),
                generation,
                difficulty,
                level: raw.level.get(index).cloned().unwrap_or_default(),
                constant: raw
                    .ds
                    .get(index)
                    .map(|value| number_decimal(value, "Diving-Fish", &raw.id, "ds"))
                    .transpose()?,
                note_designer: chart.charter.clone(),
                notes: Some(diving_notes(&chart.notes)),
                note_total: u32::try_from(diving_notes(&chart.notes).total()).ok(),
                version: raw.basic_info.version.clone(),
                regions: raw_regions(Default::default(), SourceKind::DivingFish),
                release_date: None,
                internal_id: numeric_id,
                fit_source_id: stat.map(|(id, _)| id.to_string()),
                fit_stats: stat
                    .map(|(_, value)| fit_stats(value, "Diving-Fish", &raw.id))
                    .transpose()?,
                music_id: numeric_id,
                chart_id: numeric_id,
                is_buddy: None,
                kanji: None,
                description: None,
                raw_difficulty: None,
                is_special: false,
                region_overrides: BTreeMap::new(),
                multiver_constants: BTreeMap::new(),
            })
        })
        .collect()
}

pub(super) fn dxdata(
    raw: &DxSheet,
    song_id: &SourceSongId,
    stats: &ChartStatsDocument,
) -> Result<SourceChartProjection, CatalogError> {
    build_dx(raw, song_id, SourceKind::Japan, stats)
}

pub(super) fn official(
    raw: &OfficialSheet,
    song_id: &SourceSongId,
    stats: &ChartStatsDocument,
) -> Result<SourceChartProjection, CatalogError> {
    let sheet = DxSheet {
        chart_type: raw.chart_type.clone(),
        difficulty: raw.difficulty.clone(),
        level: raw.level.clone(),
        internal_level_value: raw.internal_level_value.clone(),
        note_designer: raw.note_designer.clone(),
        note_counts: raw.note_counts.clone(),
        regions: raw.regions,
        region_overrides: BTreeMap::new(),
        is_special: false,
        version: raw.version.clone(),
        internal_id: raw.internal_id,
        release_date: None,
        multiver_internal_level_value: BTreeMap::new(),
    };
    build_dx(&sheet, song_id, SourceKind::Official, stats)
}

fn build_dx(
    raw: &DxSheet,
    song_id: &SourceSongId,
    source: SourceKind,
    stats: &ChartStatsDocument,
) -> Result<SourceChartProjection, CatalogError> {
    let label = source_id_label(song_id);
    let (generation, difficulty) = chart_identity(raw, &label)?;
    let stat_id = raw.internal_id.or_else(|| {
        numeric_source_id(song_id)
            .ok()
            .and_then(|id| fit_id(id, generation))
    });
    let stat = stat_id.and_then(|id| {
        stat_for(stats, id, difficulty_index(difficulty), &raw.level).map(|value| (id, value))
    });
    Ok(SourceChartProjection {
        source_song_id: song_id.clone(),
        generation,
        difficulty,
        level: raw.level.clone(),
        constant: raw
            .internal_level_value
            .as_ref()
            .map(|value| number_decimal(value, source.source_name(), &label, "internalLevelValue"))
            .transpose()?,
        note_designer: raw.note_designer.clone().unwrap_or_default(),
        notes: Some(dx_notes(&raw.note_counts)),
        note_total: raw
            .note_counts
            .total
            .or_else(|| u32::try_from(dx_notes(&raw.note_counts).total()).ok()),
        version: raw.version.clone(),
        regions: raw_regions(raw.regions, source),
        release_date: raw
            .release_date
            .as_deref()
            .map(|value| release_date(value, &label))
            .transpose()?,
        internal_id: raw.internal_id,
        fit_source_id: stat.map(|(id, _)| id.to_string()),
        fit_stats: stat
            .map(|(_, value)| fit_stats(value, source.source_name(), &label))
            .transpose()?,
        music_id: numeric_source_id(song_id).ok(),
        chart_id: raw.internal_id,
        is_buddy: None,
        kanji: None,
        description: None,
        raw_difficulty: matches!(
            generation,
            ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
        )
        .then_some(raw.difficulty.clone()),
        is_special: raw.is_special,
        region_overrides: region_overrides(&raw.region_overrides, source.source_name(), &label)?,
        multiver_constants: decimal_map(
            &raw.multiver_internal_level_value,
            source.source_name(),
            &label,
        )?,
    })
}

fn fit_id(base: u32, generation: ChartGeneration) -> Option<u32> {
    match generation {
        ChartGeneration::Standard => Some(base),
        ChartGeneration::Deluxe => base.checked_add(10_000),
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer => None,
    }
}

fn region_overrides(
    values: &BTreeMap<String, DxRegionOverride>,
    source: &'static str,
    song_id: &str,
) -> Result<BTreeMap<String, RegionOverride>, CatalogError> {
    values
        .iter()
        .map(|(region, value)| {
            Ok((
                region.clone(),
                RegionOverride {
                    level: value.level.clone(),
                    constant: value
                        .level_value
                        .as_ref()
                        .map(|number| {
                            number_decimal(number, source, song_id, "regionOverride.levelValue")
                        })
                        .transpose()?,
                    version: value.version.clone(),
                },
            ))
        })
        .collect()
}

fn decimal_map(
    values: &BTreeMap<String, Option<Number>>,
    source: &'static str,
    song_id: &str,
) -> Result<BTreeMap<String, Decimal>, CatalogError> {
    values
        .iter()
        .filter_map(|(key, value)| value.as_ref().map(|value| (key, value)))
        .map(|(key, value)| {
            Ok((
                key.clone(),
                number_decimal(value, source, song_id, "multiver")?,
            ))
        })
        .collect()
}

fn numeric_source_id(value: &SourceSongId) -> Result<u32, CatalogError> {
    match value.value() {
        SongIdValue::Numeric(value) => Ok(*value),
        SongIdValue::Text(_) => Err(CatalogError::InvalidSourceSongId {
            source_name: "projection",
            value: source_id_label(value),
        }),
    }
}

fn source_id_label(value: &SourceSongId) -> String {
    match value.value() {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}

fn lxns_notes(value: RawNoteCounts) -> NoteCounts {
    NoteCounts {
        tap: value.tap,
        hold: value.hold,
        slide: value.slide,
        touch: value.touch,
        break_notes: value.break_notes,
    }
}

fn dx_notes(value: &DxNoteCounts) -> NoteCounts {
    NoteCounts {
        tap: value.tap.unwrap_or_default(),
        hold: value.hold.unwrap_or_default(),
        slide: value.slide.unwrap_or_default(),
        touch: value.touch.unwrap_or_default(),
        break_notes: value.break_notes.unwrap_or_default(),
    }
}

fn diving_notes(value: &[u32]) -> NoteCounts {
    let get = |index| value.get(index).copied().unwrap_or_default();
    NoteCounts {
        tap: get(0),
        hold: get(1),
        slide: get(2),
        touch: if value.len() >= 5 { get(3) } else { 0 },
        break_notes: if value.len() >= 5 { get(4) } else { get(3) },
    }
}

const fn normal_difficulty(index: usize) -> Option<Difficulty> {
    match index {
        0 => Some(Difficulty::Basic),
        1 => Some(Difficulty::Advanced),
        2 => Some(Difficulty::Expert),
        3 => Some(Difficulty::Master),
        4 => Some(Difficulty::ReMaster),
        _ => None,
    }
}

const fn difficulty_index(value: Difficulty) -> usize {
    match value {
        Difficulty::Basic => 0,
        Difficulty::Advanced => 1,
        Difficulty::Expert => 2,
        Difficulty::Master => 3,
        Difficulty::ReMaster => 4,
        Difficulty::Utage => 0,
    }
}
