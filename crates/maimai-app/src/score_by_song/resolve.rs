use std::collections::BTreeSet;

use maimai_catalog::{CatalogQuery, SearchHit, SourceKind};
use maimai_core::{ChartGeneration, PlayerUsername, SongIdValue};

use crate::{
    identity::{IdentityQuery, MaxResults},
    scores::{Lookup, SongGenerationFilter},
};

use super::{
    PlayerLookupRequest, ScoreBySongError, ScoreBySongService, SelectedSong, SongCandidate,
    SongLookupRequest, SongSelection,
};

pub(super) struct ResolvedPlayer {
    pub lookup: Lookup,
    pub identity: Option<maimai_storage::IdentityRecord>,
}

pub(super) struct ResolvedSong {
    pub selected: SelectedSong,
    pub selection: SongSelection,
    pub generation_by_music_id: Vec<(u32, BTreeSet<ChartGeneration>)>,
    pub canonical_id: maimai_core::SourceSongId,
}

impl ScoreBySongService {
    pub(super) async fn resolve_player(
        &self,
        request: PlayerLookupRequest,
        group_id: Option<&maimai_core::GroupId>,
    ) -> Result<ResolvedPlayer, ScoreBySongError> {
        match request {
            PlayerLookupRequest::Qq(qq) => Ok(ResolvedPlayer {
                identity: self.identities.get_identity(&qq, group_id).await?,
                lookup: Lookup::Qq(qq),
            }),
            PlayerLookupRequest::Username(username) => Ok(ResolvedPlayer {
                lookup: Lookup::Username(username),
                identity: None,
            }),
            PlayerLookupRequest::Auto(target) => {
                let query = IdentityQuery::new(target.as_str())?;
                let resolution = self
                    .identities
                    .resolve_identity(
                        &query,
                        group_id,
                        MaxResults::new(20)
                            .map_err(|_| ScoreBySongError::InvalidInput { field: "target" })?,
                    )
                    .await?;
                if resolution.ambiguous {
                    return Err(ScoreBySongError::AmbiguousIdentity);
                }
                if let Some(candidate) = resolution.matches.into_iter().next() {
                    return Ok(ResolvedPlayer {
                        lookup: Lookup::Qq(candidate.identity.qq.clone()),
                        identity: Some(candidate.identity),
                    });
                }
                Ok(ResolvedPlayer {
                    lookup: Lookup::Username(
                        PlayerUsername::new(target.as_str())
                            .map_err(|_| ScoreBySongError::InvalidInput { field: "target" })?,
                    ),
                    identity: None,
                })
            }
        }
    }

    pub(super) fn resolve_song(
        &self,
        request: &SongLookupRequest,
    ) -> Result<ResolvedSong, ScoreBySongError> {
        if request.query.trim().is_empty() || request.limit == 0 || request.limit > 20 {
            return Err(ScoreBySongError::InvalidInput { field: "song" });
        }
        let snapshot = self.catalog.snapshot();
        let mut query = CatalogQuery {
            query: Some(request.query.clone()),
            ..CatalogQuery::default()
        };
        query.difficulties.extend(request.difficulty);
        extend_generation(&mut query.generations, request.generation);
        let hits = snapshot
            .query(&query)
            .map_err(|_| ScoreBySongError::Catalog)?;
        if hits.is_empty() {
            return Err(ScoreBySongError::SongNotFound);
        }
        let mut selected = None;
        for (index, hit) in hits.iter().take(request.limit).enumerate() {
            let targets = music_targets(hit, request);
            if !targets.is_empty() {
                selected = Some((index, hit, targets));
                break;
            }
        }
        let Some((selected_index, hit, targets)) = selected else {
            return Err(ScoreBySongError::MusicIdNotFound);
        };
        let candidates = hits
            .iter()
            .take(request.limit.min(5))
            .map(candidate)
            .collect::<Vec<_>>();
        let selected_candidate = candidate(hit);
        let music_ids = targets.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        Ok(ResolvedSong {
            selected: SelectedSong {
                candidate: selected_candidate,
                music_ids,
            },
            selection: SongSelection {
                auto_selected: hits.len() > 1,
                selected_rank: selected_index + 1,
                total_matches: hits.len(),
                truncated: hits.len() > request.limit,
                candidates,
            },
            generation_by_music_id: targets,
            canonical_id: hit.music.primary_id.clone(),
        })
    }
}

fn extend_generation(
    values: &mut BTreeSet<ChartGeneration>,
    generation: Option<SongGenerationFilter>,
) {
    match generation {
        Some(SongGenerationFilter::Exact(value)) => {
            values.insert(value);
        }
        Some(SongGenerationFilter::UtageAny) => {
            values.extend([
                ChartGeneration::UtageOnePlayer,
                ChartGeneration::UtageTwoPlayer,
            ]);
        }
        None => {}
    }
}

fn music_targets(
    hit: &SearchHit<'_>,
    request: &SongLookupRequest,
) -> Vec<(u32, BTreeSet<ChartGeneration>)> {
    let mut targets = Vec::<(u32, BTreeSet<ChartGeneration>)>::new();
    for chart in &hit.matched_charts {
        for source in &chart.source_matches {
            if source.song.source != SourceKind::DivingFish {
                continue;
            }
            let SongIdValue::Numeric(id) = source.song.id.value() else {
                continue;
            };
            if let Some((_, generations)) = targets.iter_mut().find(|(value, _)| *value == *id) {
                generations.insert(source.chart.generation);
            } else {
                targets.push((*id, BTreeSet::from([source.chart.generation])));
            }
        }
    }
    if targets.is_empty() {
        for source in &hit.metadata.source_projections {
            if source.source != SourceKind::DivingFish {
                continue;
            }
            let SongIdValue::Numeric(id) = source.id.value() else {
                continue;
            };
            let generations = source
                .charts
                .iter()
                .filter(|chart| {
                    request
                        .difficulty
                        .is_none_or(|difficulty| chart.difficulty == difficulty)
                        && request
                            .generation
                            .is_none_or(|generation| generation.matches(chart.generation))
                })
                .map(|chart| chart.generation)
                .collect::<BTreeSet<_>>();
            if !generations.is_empty() {
                targets.push((*id, generations));
            }
        }
    }
    targets.truncate(10);
    targets
}

fn candidate(hit: &SearchHit<'_>) -> SongCandidate {
    SongCandidate {
        id: hit.music.primary_id.clone(),
        title: hit.music.title.clone(),
        artist: hit.music.artist.clone(),
        source: source_name(hit.music.primary_id.namespace()),
        available_generations: hit
            .music
            .charts
            .iter()
            .map(|chart| chart.key.generation())
            .collect(),
        aliases: hit.music.aliases.iter().take(20).cloned().collect(),
    }
}

const fn source_name(value: maimai_core::SongIdNamespace) -> &'static str {
    match value {
        maimai_core::SongIdNamespace::Lxns => "lxns",
        maimai_core::SongIdNamespace::DivingFish => "divingfish",
        maimai_core::SongIdNamespace::OfficialCn => "official",
        maimai_core::SongIdNamespace::DxRating => "dxdata",
        maimai_core::SongIdNamespace::Yuzu => "yuzu",
    }
}
