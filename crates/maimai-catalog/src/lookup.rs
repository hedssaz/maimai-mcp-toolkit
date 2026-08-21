use std::collections::HashMap;

use thiserror::Error;

use maimai_core::{ChartKey, Music, SongIdNamespace};

use crate::{
    CatalogError, CatalogSnapshot, SourceChartMatch, SourceKind, metadata::CatalogMetadata,
};

pub(crate) fn build_chart_index(
    songs: &[Music],
    metadata: &CatalogMetadata,
) -> Result<HashMap<ChartKey, (usize, usize)>, CatalogError> {
    let mut index = HashMap::new();
    for (song_index, song) in songs.iter().enumerate() {
        for (chart_index, chart) in song.charts.iter().enumerate() {
            insert_identity(&mut index, chart.key.clone(), (song_index, chart_index))?;
        }
        for projection in &metadata.songs[song_index].source_projections {
            for source_chart in &projection.charts {
                let Some((chart_index, _)) = song.charts.iter().enumerate().find(|(_, chart)| {
                    chart.key.generation() == source_chart.generation
                        && chart.key.difficulty() == source_chart.difficulty
                }) else {
                    continue;
                };
                let alias = ChartKey::new(
                    source_chart.source_song_id.clone(),
                    source_chart.generation,
                    source_chart.difficulty,
                )
                .map_err(|_| CatalogError::InvalidSourceSongId {
                    source_name: projection.source.source_name(),
                    value: format!("{:?}", source_chart.source_song_id),
                })?;
                insert_identity(&mut index, alias, (song_index, chart_index))?;
            }
        }
    }
    Ok(index)
}

fn insert_identity(
    index: &mut HashMap<ChartKey, (usize, usize)>,
    key: ChartKey,
    target: (usize, usize),
) -> Result<(), CatalogError> {
    match index.get(&key) {
        Some(existing) if *existing != target => Err(CatalogError::AmbiguousChartIdentity { key }),
        Some(_) => Ok(()),
        None => {
            index.insert(key, target);
            Ok(())
        }
    }
}

impl CatalogSnapshot {
    pub fn source_chart(
        &self,
        key: &ChartKey,
        source: SourceKind,
    ) -> Result<Option<SourceChartMatch<'_>>, CatalogLookupError> {
        if !matches!(
            key.song().namespace(),
            SongIdNamespace::Lxns | SongIdNamespace::DivingFish
        ) {
            return Err(CatalogLookupError::UnsupportedChartNamespace {
                namespace: key.song().namespace(),
            });
        }
        let Some(&(song_index, _)) = self.chart_index.get(key) else {
            return Ok(None);
        };
        let projections = &self.metadata().songs[song_index].source_projections;
        let mut matches = projections
            .iter()
            .filter(|projection| projection.source == source)
            .flat_map(|projection| {
                projection
                    .charts
                    .iter()
                    .filter(|chart| {
                        chart.generation == key.generation() && chart.difficulty == key.difficulty()
                    })
                    .map(move |chart| SourceChartMatch {
                        song: projection,
                        chart,
                    })
            });
        let first = matches.next();
        if matches.next().is_some() {
            return Err(CatalogLookupError::AmbiguousSourceChart {
                key: key.clone(),
                source_kind: source,
            });
        }
        Ok(first)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CatalogLookupError {
    #[error("chart key namespace {namespace:?} is not a catalog-stable namespace")]
    UnsupportedChartNamespace { namespace: SongIdNamespace },

    #[error("chart {key:?} has more than one {source_kind:?} source projection")]
    AmbiguousSourceChart {
        key: ChartKey,
        source_kind: SourceKind,
    },
}
