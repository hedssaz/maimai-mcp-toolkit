use std::collections::BTreeSet;

use maimai_catalog::{CatalogQuery, CatalogSnapshot, Region, SourceKind, SourceSongProjection};
use maimai_core::{ChartConstant, ChartGeneration, ChartKey, Difficulty, SongIdValue};
use rust_decimal::Decimal;

use super::RiseScoreError;

#[derive(Clone, Debug)]
pub(super) struct CandidateSeed {
    pub key: ChartKey,
    pub display_id: SongIdValue,
    pub cover_id: SongIdValue,
    pub image_name: Option<String>,
    pub title: String,
    pub constant: ChartConstant,
    pub fit_diff: Option<Decimal>,
    pub is_current: bool,
}

pub(super) fn candidates(
    snapshot: &CatalogSnapshot,
    level: Option<&str>,
) -> Result<Vec<CandidateSeed>, RiseScoreError> {
    let query = CatalogQuery {
        level: level.map(str::to_owned),
        difficulties: BTreeSet::from([
            Difficulty::Basic,
            Difficulty::Advanced,
            Difficulty::Expert,
            Difficulty::Master,
            Difficulty::ReMaster,
        ]),
        generations: BTreeSet::from([ChartGeneration::Standard, ChartGeneration::Deluxe]),
        region_has: BTreeSet::from([Region::China]),
        ..CatalogQuery::default()
    };
    let current_versions = snapshot.current_diving_fish_versions();
    let hits = snapshot
        .query(&query)
        .map_err(RiseScoreError::CatalogQuery)?;
    let mut output = Vec::new();
    for hit in hits {
        for matched in hit.matched_charts {
            let Some(constant) = matched.chart.constant else {
                continue;
            };
            let diving_fish = diving_fish_projection(
                &hit.metadata.source_projections,
                matched.chart.key.generation(),
                matched.chart.key.difficulty(),
            );
            let source_chart = diving_fish.and_then(|source| {
                source.charts.iter().find(|chart| {
                    chart.generation == matched.chart.key.generation()
                        && chart.difficulty == matched.chart.key.difficulty()
                })
            });
            let display_id = source_chart
                .map(|chart| chart.source_song_id.value().clone())
                .unwrap_or_else(|| matched.chart.key.song().value().clone());
            output.push(CandidateSeed {
                key: matched.chart.key.clone(),
                cover_id: display_id.clone(),
                display_id,
                image_name: diving_fish.and_then(|source| source.image_name.clone()),
                title: diving_fish
                    .map_or_else(|| hit.music.title.clone(), |source| source.title.clone()),
                constant,
                fit_diff: matched.metadata.fit_diff,
                is_current: is_current_chart(
                    current_versions,
                    &hit.metadata.source_projections,
                    matched.chart.key.generation(),
                    matched.chart.key.difficulty(),
                ),
            });
        }
    }
    output.sort_by(|left, right| left.key.cmp(&right.key));
    output.dedup_by(|left, right| left.key == right.key);
    Ok(output)
}

fn diving_fish_projection(
    sources: &[SourceSongProjection],
    generation: ChartGeneration,
    difficulty: Difficulty,
) -> Option<&SourceSongProjection> {
    sources.iter().find(|source| {
        source.source == SourceKind::DivingFish
            && source
                .charts
                .iter()
                .any(|chart| chart.generation == generation && chart.difficulty == difficulty)
    })
}

pub(super) fn is_current_chart(
    current_versions: &[String],
    sources: &[SourceSongProjection],
    generation: ChartGeneration,
    difficulty: Difficulty,
) -> bool {
    sources.iter().any(|source| {
        source.source == SourceKind::DivingFish
            && current_versions.contains(&source.version)
            && source
                .charts
                .iter()
                .any(|chart| chart.generation == generation && chart.difficulty == difficulty)
    })
}
