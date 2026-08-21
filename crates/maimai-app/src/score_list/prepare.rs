use std::cmp::Reverse;

use maimai_catalog::{CatalogSnapshot, SourceKind};
use maimai_render::{SCORE_LIST_PAGE_SIZE, ScoreListItem, ScoreListView};

use crate::scores::{B50Chart, PlayerScores, best_by_chart};

use super::{ScoreListError, ScoreListTarget};

pub(super) fn prepare(
    snapshot: &CatalogSnapshot,
    scores: &PlayerScores,
    target: &ScoreListTarget,
    page: usize,
) -> Result<ScoreListView, ScoreListError> {
    if page == 0 {
        return Err(ScoreListError::invalid("page 必须是正整数"));
    }
    let best = best_by_chart(&scores.records);
    let mut records = scores
        .records
        .iter()
        .filter(|record| {
            best.get(&record.key)
                .is_some_and(|selected| std::ptr::eq(*selected, *record))
        })
        .filter(|record| matches_target(record, target))
        .collect::<Vec<_>>();
    records.sort_by_key(|record| {
        Reverse(
            record
                .achievements
                .map(|achievement| achievement.ten_thousandths()),
        )
    });
    let total = records.len();
    let pages = total.div_ceil(SCORE_LIST_PAGE_SIZE).max(1);
    if page > pages {
        return Err(ScoreListError::invalid(format!(
            "超出页数，您的成绩共计「{pages}」页，请重新输入"
        )));
    }
    let start = (page - 1) * SCORE_LIST_PAGE_SIZE;
    let end = start.saturating_add(SCORE_LIST_PAGE_SIZE).min(total);
    let items = records[start..end]
        .iter()
        .map(|record| item(snapshot, record))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ScoreListView {
        target: target.label(),
        page,
        pages,
        total,
        first: if items.is_empty() { 0 } else { start + 1 },
        items,
    })
}

fn matches_target(record: &B50Chart, target: &ScoreListTarget) -> bool {
    match target {
        ScoreListTarget::Level(level) => record.level == *level,
        ScoreListTarget::Constant(constant) => record.constant == Some(*constant),
    }
}

fn item(snapshot: &CatalogSnapshot, record: &B50Chart) -> Result<ScoreListItem, ScoreListError> {
    let projection = snapshot
        .source_chart(&record.key, SourceKind::DivingFish)?
        .ok_or_else(|| ScoreListError::MissingDivingFishChart(record.key.clone()))?;
    let max_dx_score = projection
        .chart
        .note_total
        .map(u64::from)
        .or_else(|| projection.chart.notes.map(maimai_core::NoteCounts::total))
        .and_then(|total| total.checked_mul(3))
        .and_then(|total| u32::try_from(total).ok());
    Ok(ScoreListItem {
        display_id: projection.chart.source_song_id.value().clone(),
        cover_id: projection.chart.source_song_id.value().clone(),
        image_name: projection.song.image_name.clone(),
        title: projection.song.title.clone(),
        generation: record.key.generation(),
        difficulty: record.key.difficulty(),
        constant: record.constant,
        achievement: record.achievements,
        dx_score: record.dx_score,
        max_dx_score,
        rating: record.rating,
        combo: record.full_combo,
        sync: record.full_sync,
    })
}
