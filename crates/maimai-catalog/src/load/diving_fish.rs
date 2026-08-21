use std::collections::HashMap;

use maimai_core::{
    Chart, ChartGeneration, ChartKey, Difficulty, Music, NoteCounts, SongIdNamespace, SongIdValue,
    SourceSongId,
};

use crate::{CatalogError, TextNormalizer, raw::DivingFishSong};

use super::numeric::{number_to_u32, parse_chart_constant};

pub(super) fn load_diving_fish_songs(
    source: Vec<DivingFishSong>,
    normalizer: &TextNormalizer,
    songs: &mut Vec<Music>,
    merge_ids: &mut HashMap<u32, usize>,
    title_index: &mut HashMap<String, usize>,
) -> Result<(), CatalogError> {
    for song in source {
        let raw_source_id = source_id(&song.id)?;
        let raw_numeric_id = numeric_id(&raw_source_id);
        let is_utage = raw_numeric_id.is_some_and(|song_id| song_id >= 100_000);
        let merge_id = raw_numeric_id.map(|song_id| merge_id(song_id, &song.chart_type, is_utage));
        let target = merge_id
            .and_then(|song_id| merge_ids.get(&song_id).copied())
            .or_else(|| {
                raw_numeric_id
                    .is_none()
                    .then(|| title_index.get(&normalizer.normalize(&song.title)).copied())
                    .flatten()
            });
        if let Some(index) = target {
            merge_music(&mut songs[index], song, raw_source_id, is_utage)?;
            continue;
        }
        let primary_id = raw_source_id.clone();
        let charts = charts(&song, &primary_id, &raw_source_id, is_utage)?;
        let bpm =
            number_to_u32(&song.basic_info.bpm).ok_or_else(|| CatalogError::InvalidNumber {
                source_name: "Diving-Fish",
                song_id: song.id.clone(),
                field: "bpm",
                value: song.basic_info.bpm.to_string(),
            })?;
        let index = songs.len();
        let title_key = normalizer.normalize(&song.title);
        songs.push(Music {
            primary_id,
            source_ids: vec![raw_source_id],
            title: song.title,
            artist: song.basic_info.artist,
            genre: song.basic_info.genre,
            version: song.basic_info.version,
            bpm,
            aliases: Vec::new(),
            charts,
        });
        if let Some(merge_id) = merge_id {
            merge_ids.entry(merge_id).or_insert(index);
        }
        title_index.entry(title_key).or_insert(index);
    }
    Ok(())
}

fn merge_music(
    music: &mut Music,
    song: DivingFishSong,
    raw_source_id: SourceSongId,
    is_utage: bool,
) -> Result<(), CatalogError> {
    push_unique(&mut music.source_ids, raw_source_id.clone());
    if music.artist.is_empty() {
        music.artist = song.basic_info.artist.clone();
    }
    if music.genre.is_empty() {
        music.genre = song.basic_info.genre.clone();
    }
    if music.version.is_empty() {
        music.version = song.basic_info.version.clone();
    }
    if music.bpm == 0 {
        music.bpm =
            number_to_u32(&song.basic_info.bpm).ok_or_else(|| CatalogError::InvalidNumber {
                source_name: "Diving-Fish",
                song_id: song.id.clone(),
                field: "bpm",
                value: song.basic_info.bpm.to_string(),
            })?;
    }
    for chart in charts(&song, &music.primary_id, &raw_source_id, is_utage)? {
        merge_chart(&mut music.charts, chart);
    }
    Ok(())
}

/// Diving-Fish alone owns the normal DX `+10000` ID convention.
fn merge_id(song_id: u32, chart_type: &str, is_utage: bool) -> u32 {
    if !is_utage && chart_type.eq_ignore_ascii_case("DX") && (10_000..100_000).contains(&song_id) {
        song_id - 10_000
    } else {
        song_id
    }
}

fn source_id(raw: &str) -> Result<SourceSongId, CatalogError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(CatalogError::EmptyDivingFishSongId);
    }
    let value = match value.parse::<u32>() {
        Ok(value) => SongIdValue::Numeric(value),
        Err(_) => SongIdValue::text(value).map_err(|_| CatalogError::InvalidSourceSongId {
            source_name: "Diving-Fish",
            value: raw.to_owned(),
        })?,
    };
    Ok(SourceSongId::new(SongIdNamespace::DivingFish, value))
}

fn numeric_id(source_id: &SourceSongId) -> Option<u32> {
    match source_id.value() {
        SongIdValue::Numeric(value) => Some(*value),
        SongIdValue::Text(_) => None,
    }
}

fn charts(
    song: &DivingFishSong,
    primary_id: &SourceSongId,
    raw_source_id: &SourceSongId,
    is_utage: bool,
) -> Result<Vec<Chart>, CatalogError> {
    let chart_type = song.chart_type.trim().to_ascii_uppercase();
    if !matches!(chart_type.as_str(), "DX" | "SD") {
        return Err(CatalogError::UnsupportedSourceValue {
            source_name: "Diving-Fish",
            song_id: song.id.clone(),
            field: "type",
            value: chart_type,
        });
    }
    let generation = if is_utage {
        ChartGeneration::UtageOnePlayer
    } else {
        match chart_type.as_str() {
            "DX" => ChartGeneration::Deluxe,
            "SD" => ChartGeneration::Standard,
            value => {
                return Err(CatalogError::UnsupportedSourceValue {
                    source_name: "Diving-Fish",
                    song_id: song.id.clone(),
                    field: "type",
                    value: value.to_owned(),
                });
            }
        }
    };
    let mut result = Vec::new();
    for (index, chart) in song.charts.iter().enumerate() {
        let (Some(level), Some(constant)) = (song.level.get(index), song.ds.get(index)) else {
            continue;
        };
        let difficulty = if is_utage {
            Difficulty::Utage
        } else {
            normal_difficulty(index).ok_or_else(|| CatalogError::InvalidDifficulty {
                source_name: "Diving-Fish",
                song_id: song.id.clone(),
                difficulty: index,
            })?
        };
        let key = ChartKey::new(primary_id.clone(), generation, difficulty).map_err(|_| {
            CatalogError::UnsupportedSourceValue {
                source_name: "Diving-Fish",
                song_id: song.id.clone(),
                field: "type/difficulty",
                value: format!("{generation:?}/{difficulty:?}"),
            }
        })?;
        result.push(Chart {
            key,
            source_ids: vec![raw_source_id.clone()],
            level: level.clone(),
            constant: if is_utage {
                None
            } else {
                Some(parse_chart_constant("Diving-Fish", &song.id, constant)?)
            },
            note_designer: chart.charter.clone(),
            notes: note_counts(&chart.notes),
        });
    }
    Ok(result)
}

fn merge_chart(charts: &mut Vec<Chart>, incoming: Chart) {
    let existing = charts.iter_mut().find(|chart| {
        chart.key.generation() == incoming.key.generation()
            && chart.key.difficulty() == incoming.key.difficulty()
    });
    let Some(existing) = existing else {
        charts.push(incoming);
        return;
    };
    for source_id in incoming.source_ids {
        push_unique(&mut existing.source_ids, source_id);
    }
    if existing.level.is_empty() {
        existing.level = incoming.level;
    }
    if existing.constant.is_none() {
        existing.constant = incoming.constant;
    }
    if existing.note_designer.is_empty() {
        existing.note_designer = incoming.note_designer;
    }
    if existing.notes.total() == 0 {
        existing.notes = incoming.notes;
    }
}

fn note_counts(notes: &[u32]) -> NoteCounts {
    match notes {
        [tap, hold, slide, break_notes] => NoteCounts {
            tap: *tap,
            hold: *hold,
            slide: *slide,
            touch: 0,
            break_notes: *break_notes,
        },
        [tap, hold, slide, touch, break_notes, ..] => NoteCounts {
            tap: *tap,
            hold: *hold,
            slide: *slide,
            touch: *touch,
            break_notes: *break_notes,
        },
        _ => NoteCounts::default(),
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

fn push_unique<T: Eq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}
