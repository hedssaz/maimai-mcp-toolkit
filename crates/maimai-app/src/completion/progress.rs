use std::{cmp::Reverse, collections::BTreeSet};

use maimai_catalog::{CatalogQuery, CatalogSnapshot, Region};
use maimai_render::{LevelProgressView, ProgressPage, ScoreCardCell};

use crate::scores::{B50Chart, PlayerScores, best_by_chart};

use super::{
    CompletionError, CompletionTarget, LevelProgressRequest, ProgressCategory,
    model::PROGRESS_PAGE_SIZE, records,
};

pub(crate) fn prepare(
    snapshot: &CatalogSnapshot,
    scores: &PlayerScores,
    request: &LevelProgressRequest,
) -> Result<LevelProgressView, CompletionError> {
    request.validate()?;
    let mut query = CatalogQuery {
        level: Some(request.level.trim().to_owned()),
        ..CatalogQuery::default()
    };
    if request.server == maimai_catalog::PlateServer::Cn {
        query.region_has = BTreeSet::from([Region::China]);
    }
    let records = best_by_chart(&scores.records);
    let mut completed = Vec::new();
    let mut unfinished = Vec::new();
    let mut not_started = Vec::new();
    for hit in snapshot.query(&query)? {
        for matched in &hit.matched_charts {
            let record = records.get(&matched.chart.key).copied();
            let cell = score_cell(&hit, matched, record)?;
            if record.is_none() {
                not_started.push(cell);
            } else if request
                .target
                .completed(cell.achievement, cell.combo, cell.sync)
            {
                completed.push(cell);
            } else {
                unfinished.push(cell);
            }
        }
    }
    sort_played(&mut completed, request.target);
    sort_played(&mut unfinished, request.target);
    not_started.sort_by_key(|value| Reverse(value.constant));
    let total = completed.len() + unfinished.len() + not_started.len();
    let remaining = unfinished.len() + not_started.len();
    let page = page(
        request.category,
        request.page,
        &mut completed,
        &mut unfinished,
        &mut not_started,
        remaining,
    )?;
    Ok(LevelProgressView {
        level: request.level.trim().to_owned(),
        target: request.target.label().to_owned(),
        page,
        total,
        remaining,
        completed,
        unfinished,
        not_started,
    })
}

fn score_cell(
    hit: &maimai_catalog::SearchHit<'_>,
    matched: &maimai_catalog::MatchedChart<'_>,
    record: Option<&B50Chart>,
) -> Result<ScoreCardCell, CompletionError> {
    let source = matched
        .source_matches
        .iter()
        .find(|source| source.song.source == maimai_catalog::SourceKind::DivingFish)
        .or_else(|| matched.source_matches.first());
    let cover_id = source.map_or_else(
        || hit.music.primary_id.value().clone(),
        |source| source.song.id.value().clone(),
    );
    let state = records::state(
        record,
        CompletionTarget::Achievement(super::AchievementTarget::Eighty),
    )?;
    Ok(ScoreCardCell {
        cover_id,
        image_name: source.and_then(|source| source.song.image_name.clone()),
        title: hit.music.title.clone(),
        generation: matched.chart.key.generation(),
        difficulty: matched.chart.key.difficulty(),
        level: matched.chart.level.clone(),
        constant: matched.chart.constant,
        achievement: state.achievement,
        dx_score: record.and_then(|value| value.dx_score),
        max_dx_score: u32::try_from(matched.chart.notes.total())
            .ok()
            .and_then(|value| value.checked_mul(3)),
        rating: record.and_then(|value| value.rating),
        grade: record.and_then(|value| value.grade.clone()),
        combo: state.combo,
        sync: state.sync,
    })
}

fn page(
    category: ProgressCategory,
    requested: usize,
    completed: &mut Vec<ScoreCardCell>,
    unfinished: &mut Vec<ScoreCardCell>,
    not_started: &mut Vec<ScoreCardCell>,
    remaining: usize,
) -> Result<ProgressPage, CompletionError> {
    match category {
        ProgressCategory::Overview => {
            completed.truncate(if remaining == 0 { 60 } else { 30 });
            unfinished.truncate(30);
            not_started.truncate(100);
            Ok(ProgressPage::Overview)
        }
        ProgressCategory::Completed => {
            let pages = completed.len().div_ceil(PROGRESS_PAGE_SIZE).max(1);
            retain_page(completed, requested, pages)?;
            unfinished.clear();
            not_started.clear();
            Ok(ProgressPage::Completed {
                page: requested,
                pages,
            })
        }
        ProgressCategory::Unfinished => {
            let pages = unfinished.len().div_ceil(PROGRESS_PAGE_SIZE).max(1);
            retain_page(unfinished, requested, pages)?;
            completed.clear();
            not_started.clear();
            Ok(ProgressPage::Unfinished {
                page: requested,
                pages,
            })
        }
        ProgressCategory::NotStarted => {
            completed.clear();
            unfinished.clear();
            Ok(ProgressPage::NotStarted)
        }
    }
}

pub(crate) fn retain_page<T>(
    values: &mut Vec<T>,
    page: usize,
    pages: usize,
) -> Result<(), CompletionError> {
    if page == 0 || page > pages {
        return Err(CompletionError::PageOutOfRange { pages });
    }
    let start = (page - 1) * PROGRESS_PAGE_SIZE;
    let end = (start + PROGRESS_PAGE_SIZE).min(values.len());
    values.drain(end..);
    values.drain(..start);
    Ok(())
}

fn sort_played(values: &mut [ScoreCardCell], target: CompletionTarget) {
    values.sort_by(|left, right| match target {
        CompletionTarget::Achievement(_) => right.achievement.cmp(&left.achievement),
        CompletionTarget::FullCombo(_) => right.combo.cmp(&left.combo),
        CompletionTarget::FullSync(_) => right.sync.cmp(&left.sync),
        CompletionTarget::PlateGeneral => right.achievement.cmp(&left.achievement),
        CompletionTarget::PlateExtreme | CompletionTarget::PlateGod => right.combo.cmp(&left.combo),
        CompletionTarget::PlateDance => right.sync.cmp(&left.sync),
    });
}
