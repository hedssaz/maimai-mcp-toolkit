use std::collections::{BTreeSet, HashMap};

use maimai_core::{ChartConstant, ChartGeneration, Difficulty, Music, SongIdValue, SourceSongId};

use crate::{
    CatalogError, TextNormalizer,
    metadata::{CatalogMetadata, SourceKind, SourceSongProjection},
};

use super::{PlateMember, PlateMemberIdentity, member::PlateChart};

pub(super) struct SourceIndex<'a> {
    songs: &'a [Music],
    by_df_id: HashMap<u32, Vec<(usize, &'a SourceSongProjection)>>,
    by_dx_id: HashMap<SourceSongId, (usize, &'a SourceSongProjection)>,
    by_search: HashMap<String, BTreeSet<usize>>,
    projections_by_song: HashMap<usize, Vec<&'a SourceSongProjection>>,
}

impl<'a> SourceIndex<'a> {
    pub(super) fn build(
        songs: &'a [Music],
        metadata: &'a CatalogMetadata,
        normalizer: &TextNormalizer,
    ) -> Result<Self, CatalogError> {
        let mut by_df_id = HashMap::<u32, Vec<_>>::new();
        let mut by_dx_id = HashMap::new();
        let mut by_search = HashMap::<String, BTreeSet<usize>>::new();
        let mut projections_by_song = HashMap::<usize, Vec<_>>::new();
        for (index, (song, song_metadata)) in songs.iter().zip(&metadata.songs).enumerate() {
            for value in std::iter::once(&song.title).chain(song.aliases.iter()) {
                let normalized = normalizer.normalize(value);
                if !normalized.is_empty() {
                    by_search.entry(normalized).or_default().insert(index);
                }
            }
            for projection in &song_metadata.source_projections {
                if matches!(
                    projection.source,
                    SourceKind::DivingFish | SourceKind::Japan
                ) {
                    projections_by_song
                        .entry(index)
                        .or_default()
                        .push(projection);
                }
                match projection.source {
                    SourceKind::DivingFish => {
                        if let SongIdValue::Numeric(value) = projection.id.value() {
                            by_df_id
                                .entry(*value)
                                .or_default()
                                .push((index, projection));
                        }
                    }
                    SourceKind::Japan => {
                        if let Some((previous, _)) =
                            by_dx_id.insert(projection.id.clone(), (index, projection))
                        {
                            return Err(CatalogError::AmbiguousSongIdentity {
                                source_name: "dxdata",
                                song_id: format!("{:?}", projection.id.value()),
                                title: projection.title.clone(),
                                candidates: vec![previous, index],
                            });
                        }
                    }
                    SourceKind::China | SourceKind::Official => {}
                }
            }
        }
        Ok(Self {
            songs,
            by_df_id,
            by_dx_id,
            by_search,
            projections_by_song,
        })
    }

    pub(super) fn song(&self, index: usize) -> &Music {
        &self.songs[index]
    }

    pub(super) fn df(
        &self,
        id: u32,
    ) -> Result<Option<(usize, &SourceSongProjection)>, CatalogError> {
        match self.by_df_id.get(&id).map(Vec::as_slice) {
            None | Some([]) => Ok(None),
            Some([(index, projection)]) => Ok(Some((*index, *projection))),
            Some(values) => Err(CatalogError::AmbiguousSongIdentity {
                source_name: "Diving-Fish",
                song_id: id.to_string(),
                title: values
                    .first()
                    .map_or_else(String::new, |(_, value)| value.title.clone()),
                candidates: values.iter().map(|(index, _)| *index).collect(),
            }),
        }
    }

    pub(super) fn dx(&self, id: &SourceSongId) -> Option<(usize, &SourceSongProjection)> {
        self.by_dx_id.get(id).copied()
    }

    pub(super) fn search_candidates(&self, normalized: &str) -> BTreeSet<usize> {
        self.by_search.get(normalized).cloned().unwrap_or_default()
    }

    pub(super) fn projections(&self, song_index: usize) -> &[&SourceSongProjection] {
        self.projections_by_song
            .get(&song_index)
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn df_for_song_generation(
        &self,
        song_index: usize,
        generation: ChartGeneration,
    ) -> Result<Option<&SourceSongProjection>, CatalogError> {
        let matches = self
            .by_df_id
            .values()
            .flatten()
            .filter(|(index, projection)| {
                *index == song_index
                    && projection
                        .charts
                        .iter()
                        .any(|chart| chart.generation == generation)
            })
            .map(|(_, projection)| *projection)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Ok(None),
            [only] => Ok(Some(*only)),
            _ => Err(CatalogError::AmbiguousSongIdentity {
                source_name: "Diving-Fish",
                song_id: format!("song-index:{song_index}"),
                title: self.songs[song_index].title.clone(),
                candidates: vec![song_index; matches.len()],
            }),
        }
    }
}

pub(super) fn member_from_projection(
    music: &Music,
    projection: &SourceSongProjection,
    diving_fish_projection: Option<&SourceSongProjection>,
    version: Option<&str>,
    generation: Option<ChartGeneration>,
) -> Result<PlateMember, CatalogError> {
    let generation = generation
        .or_else(|| projection_generation(projection))
        .ok_or_else(|| CatalogError::UnsupportedSourceValue {
            source_name: projection.source.source_name(),
            song_id: format!("{:?}", projection.id.value()),
            field: "charts",
            value: "没有可用于牌子的谱面".to_owned(),
        })?;
    let diving_fish_id = diving_fish_projection.and_then(|value| match value.id.value() {
        SongIdValue::Numeric(id) => Some(*id),
        SongIdValue::Text(_) => None,
    });
    let display_id =
        diving_fish_id.map_or_else(|| projection.id.value().clone(), SongIdValue::Numeric);
    let charts = projection
        .charts
        .iter()
        .filter(|chart| chart.generation == generation)
        .filter(|chart| version.is_none_or(|version| chart.version == version))
        .filter(|chart| chart.difficulty != Difficulty::Utage)
        .map(|chart| chart_from_projection(music, projection, chart))
        .collect::<Result<Vec<_>, CatalogError>>()?;
    Ok(PlateMember::new(
        PlateMemberIdentity::Catalog {
            canonical_song: music.primary_id.clone(),
            display_id,
            diving_fish_id,
        },
        projection.title.clone(),
        generation,
        projection.image_name.clone(),
        charts,
    ))
}

fn chart_from_projection(
    music: &Music,
    projection: &SourceSongProjection,
    chart: &crate::SourceChartProjection,
) -> Result<PlateChart, CatalogError> {
    let key = music
        .charts
        .iter()
        .find(|candidate| {
            candidate.key.generation() == chart.generation
                && candidate.key.difficulty() == chart.difficulty
        })
        .map(|candidate| candidate.key.clone());
    let constant = chart
        .constant
        .map(|value| ChartConstant::from_decimal_str(&value.to_string()))
        .transpose()
        .map_err(|_| CatalogError::InvalidNumber {
            source_name: projection.source.source_name(),
            song_id: format!("{:?}", projection.id.value()),
            field: "constant",
            value: chart
                .constant
                .map_or_else(String::new, |value| value.to_string()),
        })?;
    Ok(PlateChart::new(
        key,
        chart.difficulty,
        chart.level.clone(),
        constant,
    ))
}

pub(super) fn projection_generation(projection: &SourceSongProjection) -> Option<ChartGeneration> {
    projection.charts.first().map(|chart| chart.generation)
}

pub(super) fn with_image(member: PlateMember, image_name: Option<String>) -> PlateMember {
    if image_name.is_none() {
        return member;
    }
    PlateMember::new(
        member.identity().clone(),
        member.title().to_owned(),
        member.generation(),
        image_name,
        member.charts().to_vec(),
    )
}

pub(super) fn with_remaster(member: PlateMember, include: bool) -> PlateMember {
    let charts = member
        .charts()
        .iter()
        .filter(|chart| include || chart.difficulty() != Difficulty::ReMaster)
        .cloned()
        .collect();
    PlateMember::new(
        member.identity().clone(),
        member.title().to_owned(),
        member.generation(),
        member.image_name().map(str::to_owned),
        charts,
    )
}
