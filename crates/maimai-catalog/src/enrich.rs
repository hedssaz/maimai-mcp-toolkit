use std::collections::HashMap;

#[path = "enrich/parse.rs"]
mod parse;
pub(crate) use parse::{chart_identity, generation, release_date};
use parse::{
    difficulty_for_generation, difficulty_index, note_counts, number_decimal, number_u32,
    push_unique, regions, source_id_label,
};

use maimai_core::{
    Chart, ChartConstant, ChartKey, Music, SongIdNamespace, SongIdValue, SourceSongId,
};

use crate::{
    CatalogError, TextNormalizer,
    identity::SongIdentityIndex,
    metadata::{CatalogMetadata, ChartQueryMetadata},
    raw::{ChartStatsDocument, DxData, DxSheet, DxSong, TagDocument},
};

pub(crate) fn enrich_catalog(
    songs: &mut Vec<Music>,
    normalizer: &TextNormalizer,
    dxdata: &DxData,
    stats: &ChartStatsDocument,
    tags: &TagDocument,
    latest_cn_version: Option<&str>,
) -> Result<CatalogMetadata, CatalogError> {
    append_dxdata_songs(songs, normalizer, dxdata)?;
    let mut metadata = CatalogMetadata {
        songs: vec![Default::default(); songs.len()],
        charts: songs
            .iter()
            .map(|song| vec![Default::default(); song.charts.len()])
            .collect(),
        version_order: dxdata
            .versions
            .iter()
            .map(|version| version.version.clone())
            .collect(),
        ..CatalogMetadata::default()
    };
    attach_source_metadata(songs, normalizer, dxdata, latest_cn_version, &mut metadata)?;
    attach_fit_stats(songs, stats, &mut metadata)?;
    attach_tags(songs, normalizer, tags, &mut metadata)?;
    Ok(metadata)
}

fn append_dxdata_songs(
    songs: &mut Vec<Music>,
    normalizer: &TextNormalizer,
    dxdata: &DxData,
) -> Result<(), CatalogError> {
    let mut identities = SongIdentityIndex::build(songs, normalizer);
    for raw in &dxdata.songs {
        let text_id = raw.song_id.trim();
        if text_id.is_empty() {
            if normalizer.normalize(&raw.title).is_empty() {
                continue;
            }
            return Err(CatalogError::EmptyDxDataSongId);
        }
        let source_id = SourceSongId::text(SongIdNamespace::DxRating, text_id).map_err(|_| {
            CatalogError::InvalidSourceSongId {
                source_name: "dxdata",
                value: raw.song_id.clone(),
            }
        })?;
        if let Some(index) = identities.resolve_dx(raw, normalizer)? {
            merge_dx_song(&mut songs[index], raw, &source_id)?;
            continue;
        }
        let charts = raw
            .sheets
            .iter()
            .map(|sheet| dx_chart(&source_id, sheet))
            .collect::<Result<Vec<_>, _>>()?;
        let version = raw
            .sheets
            .iter()
            .find_map(|sheet| (!sheet.version.is_empty()).then_some(sheet.version.clone()))
            .unwrap_or_default();
        let index = songs.len();
        songs.push(Music {
            primary_id: source_id.clone(),
            source_ids: vec![source_id],
            title: raw.title.clone(),
            artist: raw.artist.clone(),
            genre: raw.category.clone(),
            version,
            bpm: raw
                .bpm
                .as_ref()
                .map(|value| number_u32(value, "dxdata", &raw.song_id, "bpm"))
                .transpose()?
                .map_or(0, |value| value),
            aliases: raw
                .song_id
                .ne(&raw.title)
                .then_some(raw.song_id.clone())
                .into_iter()
                .collect(),
            charts,
        });
        identities.insert(index, &songs[index], normalizer);
    }
    Ok(())
}

fn merge_dx_song(
    music: &mut Music,
    raw: &DxSong,
    source_id: &SourceSongId,
) -> Result<(), CatalogError> {
    push_unique(&mut music.source_ids, source_id.clone());
    for sheet in &raw.sheets {
        let (generation, difficulty) = chart_identity(sheet, &raw.song_id)?;
        if let Some(chart) = music.charts.iter_mut().find(|chart| {
            chart.key.generation() == generation && chart.key.difficulty() == difficulty
        }) {
            if let Some(internal_id) = sheet.internal_id {
                push_unique(
                    &mut chart.source_ids,
                    SourceSongId::numeric(SongIdNamespace::DxRating, internal_id),
                );
            }
            continue;
        }
        music.charts.push(dx_chart(&music.primary_id, sheet)?);
    }
    Ok(())
}

fn dx_chart(song_id: &SourceSongId, sheet: &DxSheet) -> Result<Chart, CatalogError> {
    let source_ids = sheet
        .internal_id
        .map(|id| vec![SourceSongId::numeric(SongIdNamespace::DxRating, id)])
        .unwrap_or_else(|| vec![song_id.clone()]);
    let (generation, difficulty) = chart_identity(sheet, &source_id_label(song_id))?;
    let key = ChartKey::new(song_id.clone(), generation, difficulty).map_err(|_| {
        CatalogError::UnsupportedSourceValue {
            source_name: "dxdata",
            song_id: source_id_label(song_id),
            field: "type/difficulty",
            value: format!("{generation:?}/{difficulty:?}"),
        }
    })?;
    Ok(Chart {
        key,
        source_ids,
        level: sheet.level.clone(),
        constant: sheet
            .internal_level_value
            .as_ref()
            .map(|value| {
                ChartConstant::from_decimal_str(&value.to_string()).map_err(|_| {
                    CatalogError::InvalidNumber {
                        source_name: "dxdata",
                        song_id: format!("{:?}", song_id.value()),
                        field: "internalLevelValue",
                        value: value.to_string(),
                    }
                })
            })
            .transpose()?,
        note_designer: sheet.note_designer.clone().unwrap_or_default(),
        notes: note_counts(&sheet.note_counts),
    })
}

fn attach_source_metadata(
    songs: &[Music],
    normalizer: &TextNormalizer,
    dxdata: &DxData,
    latest_cn_version: Option<&str>,
    metadata: &mut CatalogMetadata,
) -> Result<(), CatalogError> {
    let identities = SongIdentityIndex::build(songs, normalizer);
    for raw in &dxdata.songs {
        let Some(song_index) = identities.resolve_dx(raw, normalizer)? else {
            continue;
        };
        let song = &songs[song_index];
        let song_meta = &mut metadata.songs[song_index];
        song_meta
            .source_labels
            .insert(crate::metadata::SourceKind::Japan);
        song_meta.is_new_jp |= raw.is_new;
        song_meta.is_locked = Some(song_meta.is_locked.unwrap_or(false) || raw.is_locked);
        for sheet in &raw.sheets {
            let (generation, difficulty) = chart_identity(sheet, &raw.song_id)?;
            let Some(chart_index) = song.charts.iter().position(|chart| {
                chart.key.generation() == generation && chart.key.difficulty() == difficulty
            }) else {
                continue;
            };
            let chart_meta = &mut metadata.charts[song_index][chart_index];
            chart_meta.version = (!sheet.version.is_empty()).then_some(sheet.version.clone());
            chart_meta.release_date = sheet
                .release_date
                .as_deref()
                .map(|value| release_date(value, &raw.song_id))
                .transpose()?;
            let mut dx_regions = regions(sheet.regions);
            dx_regions.cn = false;
            dx_regions.jp = true;
            chart_meta.regions.merge(dx_regions);
            for (version, value) in &sheet.multiver_internal_level_value {
                if let Some(value) = value {
                    chart_meta.multiver_constants.insert(
                        version.clone(),
                        number_decimal(value, "dxdata", &raw.song_id, "multiver")?,
                    );
                }
            }
            song_meta.regions.merge(chart_meta.regions);
        }
    }
    for (song_index, song) in songs.iter().enumerate() {
        let song_meta = &mut metadata.songs[song_index];
        if song
            .source_ids
            .iter()
            .any(|id| id.namespace() == SongIdNamespace::Lxns)
        {
            song_meta
                .source_labels
                .insert(crate::metadata::SourceKind::China);
            song_meta.regions.cn = true;
            song_meta.is_new_cn = latest_cn_version == Some(song.version.as_str());
        }
        if song
            .source_ids
            .iter()
            .any(|id| id.namespace() == SongIdNamespace::DivingFish)
        {
            song_meta
                .source_labels
                .insert(crate::metadata::SourceKind::DivingFish);
            song_meta.regions.cn = true;
        }
        apply_cn_chart_regions(song, &mut metadata.charts[song_index]);
    }
    Ok(())
}

fn attach_fit_stats(
    songs: &[Music],
    stats: &ChartStatsDocument,
    metadata: &mut CatalogMetadata,
) -> Result<(), CatalogError> {
    for (song_index, song) in songs.iter().enumerate() {
        for (chart_index, chart) in song.charts.iter().enumerate() {
            let Some(id) = chart.source_ids.iter().find_map(|id| match id.value() {
                SongIdValue::Numeric(value) if id.namespace() == SongIdNamespace::DivingFish => {
                    Some(*value)
                }
                _ => None,
            }) else {
                continue;
            };
            let Some(stat) = stats
                .charts
                .get(&id.to_string())
                .and_then(|values| values.get(difficulty_index(chart.key.difficulty())))
            else {
                continue;
            };
            let Some(fit_diff) = stat.fit_diff.as_ref() else {
                continue;
            };
            if stat.diff.as_ref().is_some_and(|value| {
                !value
                    .trim()
                    .trim_end_matches('?')
                    .eq_ignore_ascii_case(chart.level.trim().trim_end_matches('?'))
            }) {
                continue;
            }
            metadata.charts[song_index][chart_index].fit_diff = Some(number_decimal(
                fit_diff,
                "Diving-Fish",
                &id.to_string(),
                "fit_diff",
            )?);
        }
    }
    Ok(())
}

fn attach_tags(
    songs: &[Music],
    normalizer: &TextNormalizer,
    tags: &TagDocument,
    metadata: &mut CatalogMetadata,
) -> Result<(), CatalogError> {
    for tag in &tags.tags {
        for label in tag.localized_name.values() {
            metadata
                .tag_names
                .insert(normalizer.normalize(label), tag.id);
        }
        metadata.tag_names.insert(tag.id.to_string(), tag.id);
        if let Some(label) = tag
            .localized_name
            .get("zh-Hans")
            .or_else(|| tag.localized_name.get("en"))
        {
            metadata.tag_labels.insert(tag.id, label.clone());
        }
    }
    let song_titles = songs.iter().enumerate().fold(
        HashMap::<String, Vec<usize>>::new(),
        |mut values, (index, song)| {
            values
                .entry(normalizer.normalize(&song.title))
                .or_default()
                .push(index);
            values
        },
    );
    for tagged in &tags.tag_songs {
        let Some(song_indices) = song_titles.get(&normalizer.normalize(&tagged.song_id)) else {
            continue;
        };
        let generation = generation(&tagged.sheet_type, &tagged.song_id)?;
        let difficulty =
            difficulty_for_generation(generation, &tagged.sheet_difficulty, &tagged.song_id)?;
        for song_index in song_indices {
            if let Some(chart_index) = songs[*song_index].charts.iter().position(|chart| {
                chart.key.generation() == generation && chart.key.difficulty() == difficulty
            }) {
                metadata.charts[*song_index][chart_index]
                    .tag_ids
                    .insert(tagged.tag_id);
            }
        }
    }
    Ok(())
}

fn apply_cn_chart_regions(song: &Music, metadata: &mut [ChartQueryMetadata]) {
    for (chart, chart_meta) in song.charts.iter().zip(metadata) {
        if chart.source_ids.iter().any(|id| {
            matches!(
                id.namespace(),
                SongIdNamespace::Lxns | SongIdNamespace::DivingFish
            )
        }) {
            chart_meta.regions.cn = true;
        }
    }
}
