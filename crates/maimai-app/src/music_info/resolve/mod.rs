mod search;
mod view;

use std::collections::BTreeSet;

use maimai_catalog::{CatalogSnapshot, SearchHit};
use maimai_core::{ChartGeneration, ChartKey};
use maimai_render::MusicInfoView;

use super::{MusicInfoChartType, MusicInfoError, MusicInfoRequest};
use search::select_hit;
use view::{partial, prepare};

pub(crate) struct PreparedMusicInfo {
    pub view: MusicInfoView,
    pub chart_keys: Vec<ChartKey>,
    pub generation: ChartGeneration,
    pub current: bool,
    pub chart_type: Option<MusicInfoChartType>,
    pub query: String,
    pub music_id: String,
    pub title: String,
}

pub(super) fn resolve(
    snapshot: &CatalogSnapshot,
    request: &MusicInfoRequest,
) -> Result<Vec<PreparedMusicInfo>, MusicInfoError> {
    let Some((hit, generations)) = catalog_variants(snapshot, request)? else {
        return partial(request).map(|value| vec![value]);
    };
    generations
        .into_iter()
        .map(|generation| prepare(snapshot, request, &hit, generation))
        .collect()
}

pub(crate) fn catalog_variants<'a>(
    snapshot: &'a CatalogSnapshot,
    request: &MusicInfoRequest,
) -> Result<Option<(SearchHit<'a>, Vec<ChartGeneration>)>, MusicInfoError> {
    let Some((hit, inferred)) = select_hit(snapshot, request)? else {
        return Ok(None);
    };
    let requested = request
        .chart_type
        .or(inferred)
        .map(MusicInfoChartType::generation);
    let mut available = hit
        .matched_charts
        .iter()
        .map(|chart| chart.chart.key.generation())
        .collect::<BTreeSet<_>>();
    if let Some(hint) = &request.resolved
        && !hint.chart_types.is_empty()
    {
        available.retain(|generation| hint.chart_types.contains(generation));
    }
    let generations = generations(&available, requested, &request.query_label())?;
    Ok(Some((hit, generations)))
}

pub(crate) fn prepare_catalog_variant(
    snapshot: &CatalogSnapshot,
    request: &MusicInfoRequest,
    hit: &SearchHit<'_>,
    generation: ChartGeneration,
) -> Result<PreparedMusicInfo, MusicInfoError> {
    prepare(snapshot, request, hit, generation)
}

fn generations(
    available: &BTreeSet<ChartGeneration>,
    requested: Option<ChartGeneration>,
    query: &str,
) -> Result<Vec<ChartGeneration>, MusicInfoError> {
    if let Some(requested) = requested {
        return available
            .contains(&requested)
            .then_some(vec![requested])
            .ok_or_else(|| MusicInfoError::NotFound(query.to_owned()));
    }
    let mut normal = [ChartGeneration::Standard, ChartGeneration::Deluxe]
        .into_iter()
        .filter(|generation| available.contains(generation))
        .collect::<Vec<_>>();
    if normal.is_empty()
        && let Some(utage) = [
            ChartGeneration::UtageOnePlayer,
            ChartGeneration::UtageTwoPlayer,
        ]
        .into_iter()
        .find(|generation| available.contains(generation))
    {
        normal.push(utage);
    }
    if normal.is_empty() {
        Err(MusicInfoError::NotFound(query.to_owned()))
    } else {
        Ok(normal)
    }
}
