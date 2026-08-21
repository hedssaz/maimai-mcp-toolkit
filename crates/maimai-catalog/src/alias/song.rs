use std::collections::{BTreeMap, BTreeSet};

use maimai_core::{ChartGeneration, SongIdValue};

use crate::{
    CatalogFiles, CatalogQuery, CatalogSnapshot, SearchHit, SourceKind, SourceSongProjection,
};

use super::{
    AddAliasOutcome, AddAliasRequest, AddAliasResult, AliasError, AliasKind, AliasListRequest,
    AliasListResult, AliasSong, DeleteAliasRequest, DeleteAliasResult, SongAliasEntry,
    SongAliasMutation, SongAliasTarget, file,
};

pub(super) fn add(
    files: &CatalogFiles,
    snapshot: &CatalogSnapshot,
    request: AddAliasRequest,
) -> Result<AddAliasResult, AliasError> {
    let song = resolve(snapshot, request.song_target()?)?;
    let (path, mut document) = file::read(&files.custom_aliases, AliasKind::Song, true)?;
    let song_key = id_value(&song.song_id);
    let aliases = document.entry(song_key.clone()).or_default();
    let normalized = snapshot.normalizer().normalize(request.alias().as_str());
    let existing = aliases
        .iter()
        .any(|value| snapshot.normalizer().normalize(value) == normalized);
    let outcome = if existing {
        AddAliasOutcome::AlreadyExists
    } else {
        aliases.push(request.alias().as_str().to_owned());
        file::write_atomic(&path, &document)?;
        AddAliasOutcome::Added
    };
    let aliases = document
        .get(&song_key)
        .into_iter()
        .flatten()
        .map(|value| snapshot.to_simplified(value))
        .collect();
    Ok(AddAliasResult::Song {
        outcome,
        value: SongAliasMutation {
            song,
            alias: request.alias().as_str().to_owned(),
            aliases,
            document: files.custom_aliases.clone(),
        },
    })
}

pub(super) fn delete(
    files: &CatalogFiles,
    snapshot: &CatalogSnapshot,
    request: DeleteAliasRequest,
) -> Result<DeleteAliasResult, AliasError> {
    let song = resolve(snapshot, request.song_target()?)?;
    let (path, mut document) = file::read(&files.custom_aliases, AliasKind::Song, false)?;
    let song_key = id_value(&song.song_id);
    let aliases = document
        .get_mut(&song_key)
        .filter(|values| !values.is_empty())
        .ok_or_else(|| AliasError::NoSongAliases {
            song_id: song_key.clone(),
        })?;
    let normalized = snapshot.normalizer().normalize(request.alias().as_str());
    let position = aliases
        .iter()
        .position(|value| snapshot.normalizer().normalize(value) == normalized)
        .ok_or_else(|| AliasError::SongAliasNotFound {
            alias: request.alias().as_str().to_owned(),
            song_id: song_key.clone(),
        })?;
    let removed = aliases.remove(position);
    let remaining = aliases
        .iter()
        .map(|value| snapshot.to_simplified(value))
        .collect::<Vec<_>>();
    if aliases.is_empty() {
        document.remove(&song_key);
    }
    file::write_atomic(&path, &document)?;
    Ok(DeleteAliasResult::Song(SongAliasMutation {
        song,
        alias: removed,
        aliases: remaining,
        document: files.custom_aliases.clone(),
    }))
}

pub(super) fn list(
    snapshot: &CatalogSnapshot,
    request: AliasListRequest,
) -> Result<AliasListResult, AliasError> {
    let query = request
        .query()
        .ok_or(AliasError::SongListQueryRequired)?
        .to_owned();
    let mut hits = snapshot.query(&CatalogQuery {
        query: Some(query.clone()),
        limit: None,
        ..CatalogQuery::default()
    })?;
    let total_matches = hits.len();
    hits.truncate(request.limit());
    let entries = hits
        .iter()
        .map(|hit| SongAliasEntry {
            song: from_hit(hit),
            aliases: display_aliases(snapshot, &hit.music.aliases),
        })
        .collect();
    Ok(AliasListResult::Songs {
        query,
        limit: request.limit(),
        total_matches,
        truncated: total_matches > request.limit(),
        entries,
    })
}

fn resolve(snapshot: &CatalogSnapshot, target: &SongAliasTarget) -> Result<AliasSong, AliasError> {
    let mut matches = snapshot
        .songs()
        .iter()
        .enumerate()
        .filter(|(index, song)| {
            let Some(metadata) = snapshot.song_metadata(*index) else {
                return false;
            };
            match target {
                SongAliasTarget::Id(SongIdValue::Numeric(expected)) => {
                    metadata.canonical_numeric_ids.contains(expected)
                }
                SongAliasTarget::Id(SongIdValue::Text(expected)) => metadata
                    .source_projections
                    .iter()
                    .any(|source| id_value(source.id.value()) == expected.as_str()),
                SongAliasTarget::Title(expected) => {
                    let expected = snapshot.normalizer().normalize(expected.as_str());
                    snapshot.normalizer().normalize(&song.title) == expected
                        || metadata.source_projections.iter().any(|source| {
                            snapshot.normalizer().normalize(&source.title) == expected
                        })
                }
            }
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(AliasError::SongNotFound),
        1 => {
            let index = matches.remove(0);
            let hit = SearchHit {
                music: &snapshot.songs()[index],
                metadata: snapshot
                    .song_metadata(index)
                    .ok_or(AliasError::SongNotFound)?,
                matched_by: crate::MatchKind::FilterOnly,
                matched_value: None,
                matched_charts: Vec::new(),
                song_index: index,
            };
            Ok(from_hit(&hit))
        }
        _ => Err(AliasError::AmbiguousSong {
            target: target_label(target),
        }),
    }
}

fn from_hit(hit: &SearchHit<'_>) -> AliasSong {
    let mut projections = hit.metadata.source_projections.iter().collect::<Vec<_>>();
    projections.sort_by_key(|source| source_priority(source.source));
    let primary = projections.first().copied();
    let song_id = primary.map_or_else(|| hit.music.primary_id.value().clone(), canonical_source_id);
    let primary_source_id =
        primary.map_or_else(|| hit.music.primary_id.clone(), |source| source.id.clone());
    let mut source_ids = BTreeMap::new();
    let mut source_labels = Vec::new();
    for source in projections {
        source_ids
            .entry(source.source)
            .or_insert_with(|| source.id.clone());
        if !source_labels.contains(&source.source) {
            source_labels.push(source.source);
        }
    }
    AliasSong {
        song_id,
        source_id: primary_source_id,
        source_ids,
        title: primary.map_or_else(|| hit.music.title.clone(), |source| source.title.clone()),
        artist: primary.map_or_else(|| hit.music.artist.clone(), |source| source.artist.clone()),
        source_labels,
    }
}

fn display_aliases(snapshot: &CatalogSnapshot, values: &[String]) -> Vec<String> {
    let mut output = Vec::new();
    let mut normalized = BTreeSet::new();
    for value in values {
        let display = snapshot.to_simplified(value);
        if normalized.insert(snapshot.normalizer().normalize(&display)) {
            output.push(display);
        }
    }
    output
}

fn canonical_source_id(source: &SourceSongProjection) -> SongIdValue {
    match (source.id.value(), source.source) {
        (SongIdValue::Numeric(value), SourceKind::DivingFish)
            if (10_000..100_000).contains(value)
                && source
                    .charts
                    .iter()
                    .any(|chart| chart.generation == ChartGeneration::Deluxe) =>
        {
            SongIdValue::Numeric(value - 10_000)
        }
        _ => source.id.value().clone(),
    }
}

fn id_value(value: &SongIdValue) -> String {
    match value {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}

const fn source_priority(value: SourceKind) -> usize {
    match value {
        SourceKind::China => 0,
        SourceKind::Official => 1,
        SourceKind::Japan => 2,
        SourceKind::DivingFish => 3,
    }
}

fn target_label(target: &SongAliasTarget) -> String {
    match target {
        SongAliasTarget::Id(SongIdValue::Numeric(value)) => value.to_string(),
        SongAliasTarget::Id(SongIdValue::Text(value)) => value.as_str().to_owned(),
        SongAliasTarget::Title(value) => value.as_str().to_owned(),
    }
}
