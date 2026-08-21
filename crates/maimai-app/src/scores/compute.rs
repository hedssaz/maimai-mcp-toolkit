use std::{cmp::Ordering, collections::BTreeMap};

use maimai_catalog::CatalogSnapshot;
use maimai_core::{
    ChartGeneration, RatingBreakdown, ScoreSource, b50_rating_breakdown, single_song_rating,
};

use super::{
    B50Chart, B50Computation, B50Result, Lookup, PlayerScoreProfile, PlayerScores, RatingMode,
    ScoreError, SongFilter, catalog::ScoreCatalog,
};

pub(super) struct UpstreamTotals {
    pub b35: Option<u32>,
    pub b15: Option<u32>,
    pub total: Option<u32>,
}

pub fn compute_b50_from_records(
    scores: &PlayerScores,
    mode: RatingMode,
) -> Result<B50Result, ScoreError> {
    let mut computation = B50Computation {
        input: scores.records.len(),
        ..B50Computation::default()
    };
    let mut unique = BTreeMap::new();
    for record in &scores.records {
        let Some(candidate) = rated_candidate(record, mode, &mut computation)? else {
            continue;
        };
        match unique.entry(candidate.key.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                computation.duplicate_lower_rating += 1;
                if chart_order(&candidate, entry.get()) == Ordering::Less {
                    entry.insert(candidate);
                }
            }
        }
    }
    computation.eligible = unique.len();
    let (mut b15, mut b35): (Vec<_>, Vec<_>) =
        unique.into_values().partition(|chart| chart.is_current);
    sort_and_truncate(&mut b35, 35);
    sort_and_truncate(&mut b15, 15);
    let rating_breakdown = breakdown(&b35, &b15)?;
    let fit_index = super::compute_fit_index(&b35, &b15);
    let mut player = scores.player.clone();
    player.actual_rating = player.rating;
    player.rating = Some(rating_breakdown.total);
    Ok(B50Result {
        lookup: scores.lookup.clone(),
        source: scores.source,
        player,
        rating_breakdown,
        b35,
        b15,
        mode,
        computation: Some(computation),
        fit_index,
    })
}

pub fn filter_single_song(
    scores: &PlayerScores,
    filter: &SongFilter,
    snapshot: &CatalogSnapshot,
) -> Result<Vec<B50Chart>, ScoreError> {
    let canonical = ScoreCatalog::new(snapshot)?.canonical_song(&filter.song)?;
    let mut matches = scores
        .records
        .iter()
        .filter(|record| record.key.song() == &canonical)
        .filter(|record| {
            filter
                .generation
                .is_none_or(|generation| generation.matches(record.key.generation()))
        })
        .filter(|record| {
            filter
                .difficulty
                .is_none_or(|difficulty| record.key.difficulty() == difficulty)
        })
        .cloned()
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(matches)
}

pub(super) fn b50_from_sections(
    lookup: Lookup,
    source: ScoreSource,
    player: PlayerScoreProfile,
    mut b35: Vec<B50Chart>,
    mut b15: Vec<B50Chart>,
    upstream: UpstreamTotals,
) -> Result<B50Result, ScoreError> {
    sort_and_truncate(&mut b35, 35);
    sort_and_truncate(&mut b15, 15);
    let computed = breakdown(&b35, &b15)?;
    let b35_rating = upstream.b35.unwrap_or(computed.b35);
    let b15_rating = upstream.b15.unwrap_or(computed.b15);
    let total = match upstream.total {
        Some(total) if upstream.b35.is_some() && upstream.b15.is_some() => total,
        _ => b35_rating
            .checked_add(b15_rating)
            .ok_or(maimai_core::RatingError::ArithmeticOverflow)?,
    };
    let fit_index = super::compute_fit_index(&b35, &b15);
    Ok(B50Result {
        lookup,
        source,
        player,
        rating_breakdown: RatingBreakdown {
            b35: b35_rating,
            b15: b15_rating,
            total,
        },
        b35,
        b15,
        mode: RatingMode::Actual,
        computation: None,
        fit_index,
    })
}

fn rated_candidate(
    record: &B50Chart,
    mode: RatingMode,
    computation: &mut B50Computation,
) -> Result<Option<B50Chart>, ScoreError> {
    if matches!(
        record.key.generation(),
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
    ) {
        computation.skipped_utage += 1;
        return Ok(None);
    }
    let mut candidate = record.clone();
    match mode {
        RatingMode::Actual => {
            if candidate.rating.is_none() {
                computation.skipped_missing_rating += 1;
                return Ok(None);
            }
        }
        RatingMode::Fit => {
            let Some((constant, achievements)) = candidate
                .fit_constant
                .zip(candidate.achievements.and_then(|value| value.ranked()))
            else {
                computation.skipped_missing_fit += 1;
                return Ok(None);
            };
            candidate.original_rating = candidate.rating;
            candidate.rating = Some(single_song_rating(constant, achievements)?);
        }
    }
    Ok(Some(candidate))
}

fn sort_and_truncate(values: &mut Vec<B50Chart>, limit: usize) {
    values.sort_by(chart_order);
    values.truncate(limit);
}

fn chart_order(left: &B50Chart, right: &B50Chart) -> Ordering {
    right
        .rating
        .cmp(&left.rating)
        .then_with(|| {
            right
                .achievements
                .and_then(|value| value.ranked())
                .cmp(&left.achievements.and_then(|value| value.ranked()))
        })
        .then_with(|| {
            right
                .fit_constant
                .or(right.constant)
                .cmp(&left.fit_constant.or(left.constant))
        })
        .then_with(|| left.key.cmp(&right.key))
}

fn breakdown(b35: &[B50Chart], b15: &[B50Chart]) -> Result<RatingBreakdown, ScoreError> {
    let b35 = b35
        .iter()
        .filter_map(|chart| chart.rating)
        .collect::<Vec<_>>();
    let b15 = b15
        .iter()
        .filter_map(|chart| chart.rating)
        .collect::<Vec<_>>();
    b50_rating_breakdown(&b35, &b15).map_err(ScoreError::from)
}

#[cfg(test)]
mod tests {
    use maimai_core::{
        ChartGeneration, ChartKey, Difficulty, PlayAchievement, QqId, ScoreSource, SongIdNamespace,
        SourceSongId, UtageScore,
    };

    use super::compute_b50_from_records;
    use crate::scores::{B50Chart, Lookup, PlayerScoreProfile, PlayerScores, RatingMode};

    #[test]
    fn ordinary_b50_excludes_utage_without_discarding_its_value()
    -> Result<(), Box<dyn std::error::Error>> {
        let achievement = PlayAchievement::from(UtageScore::from_ten_thousandths(1_535_756));
        let key = ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::Lxns, 111_597),
            ChartGeneration::UtageOnePlayer,
            Difficulty::Utage,
        )?;
        let scores = PlayerScores {
            lookup: Lookup::Qq(QqId::new("10001")?),
            source: ScoreSource::Local,
            player: PlayerScoreProfile::default(),
            records: vec![B50Chart {
                source_song_id: key.song().clone(),
                key,
                title: "[息]ノンブレス・オブリージュ".to_owned(),
                level: "13+?".to_owned(),
                constant: None,
                achievements: Some(achievement),
                dx_score: Some(2_295),
                rating: Some(9_999),
                original_rating: None,
                grade: None,
                full_combo: None,
                full_sync: None,
                version: String::new(),
                is_current: true,
                fit_constant: None,
                fit_label: None,
            }],
        };

        assert_eq!(scores.records[0].achievements, Some(achievement));
        let result = compute_b50_from_records(&scores, RatingMode::Actual)?;
        assert!(result.b35.is_empty());
        assert!(result.b15.is_empty());
        assert_eq!(result.rating_breakdown.total, 0);
        assert_eq!(result.computation.map(|value| value.skipped_utage), Some(1));
        Ok(())
    }
}
