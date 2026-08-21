use std::collections::HashMap;

use maimai_core::{
    Chart, ChartGeneration, ChartKey, Difficulty, Music, NoteCounts, SongIdNamespace, SourceSongId,
};

use crate::{
    CatalogError, TextNormalizer,
    raw::{LxnsCatalog, LxnsChart, LxnsSong, RawNoteCounts},
};

use super::numeric::{number_to_u32, parse_chart_constant};

pub(super) fn load_lxns_songs(
    catalog: LxnsCatalog,
    normalizer: &TextNormalizer,
    songs: &mut Vec<Music>,
    merge_ids: &mut HashMap<u32, usize>,
    title_index: &mut HashMap<String, usize>,
) -> Result<(), CatalogError> {
    let genre_titles = catalog
        .genres
        .into_iter()
        .map(|genre| (genre.genre, genre.title))
        .collect::<HashMap<_, _>>();
    let version_titles = catalog
        .versions
        .into_iter()
        .map(|version| (version.version, version.title))
        .collect::<HashMap<_, _>>();

    for song in catalog.songs {
        let source_id = SourceSongId::numeric(SongIdNamespace::Lxns, song.id);
        let charts = lxns_charts(&song, &source_id)?;
        let genre = genre_titles.get(&song.genre).cloned().unwrap_or(song.genre);
        let version = version_titles
            .get(&song.version)
            .cloned()
            .unwrap_or_else(|| song.version.to_string());
        let bpm = number_to_u32(&song.bpm).ok_or_else(|| CatalogError::InvalidNumber {
            source_name: "LXNS",
            song_id: song.id.to_string(),
            field: "bpm",
            value: song.bpm.to_string(),
        })?;
        let index = songs.len();
        let title_key = normalizer.normalize(&song.title);
        songs.push(Music {
            primary_id: source_id.clone(),
            source_ids: vec![source_id],
            title: song.title,
            artist: song.artist,
            genre,
            version,
            bpm,
            aliases: Vec::new(),
            charts,
        });
        merge_ids.entry(song.id).or_insert(index);
        title_index.entry(title_key).or_insert(index);
    }
    Ok(())
}

fn lxns_charts(song: &LxnsSong, source_id: &SourceSongId) -> Result<Vec<Chart>, CatalogError> {
    let mut charts = Vec::new();
    append_charts(
        &mut charts,
        song.id,
        source_id,
        &song.difficulties.standard,
        ChartGeneration::Standard,
    )?;
    append_charts(
        &mut charts,
        song.id,
        source_id,
        &song.difficulties.dx,
        ChartGeneration::Deluxe,
    )?;
    for chart in &song.difficulties.utage {
        let generation = if chart.is_buddy {
            ChartGeneration::UtageTwoPlayer
        } else {
            ChartGeneration::UtageOnePlayer
        };
        charts.push(build_chart(
            song.id,
            source_id,
            chart,
            generation,
            Difficulty::Utage,
        )?);
    }
    Ok(charts)
}

fn append_charts(
    destination: &mut Vec<Chart>,
    song_id: u32,
    source_id: &SourceSongId,
    charts: &[LxnsChart],
    generation: ChartGeneration,
) -> Result<(), CatalogError> {
    for chart in charts {
        let difficulty = normal_difficulty(usize::from(chart.difficulty)).ok_or(
            CatalogError::InvalidDifficulty {
                source_name: "LXNS",
                song_id: song_id.to_string(),
                difficulty: usize::from(chart.difficulty),
            },
        )?;
        destination.push(build_chart(
            song_id, source_id, chart, generation, difficulty,
        )?);
    }
    Ok(())
}

fn build_chart(
    song_id: u32,
    source_id: &SourceSongId,
    chart: &LxnsChart,
    generation: ChartGeneration,
    difficulty: Difficulty,
) -> Result<Chart, CatalogError> {
    let key = ChartKey::new(source_id.clone(), generation, difficulty).map_err(|_| {
        CatalogError::UnsupportedSourceValue {
            source_name: "LXNS",
            song_id: song_id.to_string(),
            field: "type/difficulty",
            value: format!("{generation:?}/{difficulty:?}"),
        }
    })?;
    Ok(Chart {
        key,
        source_ids: vec![source_id.clone()],
        level: chart.level.clone(),
        constant: if difficulty == Difficulty::Utage {
            None
        } else {
            Some(parse_chart_constant(
                "LXNS",
                &song_id.to_string(),
                &chart.level_value,
            )?)
        },
        note_designer: chart.note_designer.clone(),
        notes: note_counts(chart.notes),
    })
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

const fn note_counts(notes: RawNoteCounts) -> NoteCounts {
    NoteCounts {
        tap: notes.tap,
        hold: notes.hold,
        slide: notes.slide,
        touch: notes.touch,
        break_notes: notes.break_notes,
    }
}
