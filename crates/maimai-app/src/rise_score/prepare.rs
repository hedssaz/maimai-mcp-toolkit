use std::collections::{HashMap, HashSet};

use maimai_catalog::CatalogSnapshot;
use maimai_core::{AchievementRate, ChartKey, single_song_rating};
use maimai_render::{RiseScoreCandidate, RiseScoreSection, RiseScoreView};
use rust_decimal::Decimal;

use crate::scores::{B50Chart, PlayerScores, RatingMode, best_by_chart, compute_b50_from_records};

use super::{
    RiseScoreAlgorithm, RiseScoreError,
    catalog::{CandidateSeed, candidates},
    expected::{
        ExpectedScoreInput, candidate_score, margin_limit, probability, target_abilities,
        target_allowed,
    },
    rng::RiseRandom,
    selection::{ExpectedPick, expected_better, fit_bucket, select_expected, select_legacy},
};

struct Section<'a> {
    kind: RiseScoreSection,
    records: &'a [B50Chart],
    other: &'a [B50Chart],
    capacity: usize,
}

pub(super) fn prepare(
    snapshot: &CatalogSnapshot,
    scores: &PlayerScores,
    level: Option<&str>,
    score: Option<u32>,
    algorithm: RiseScoreAlgorithm,
    rng: &mut impl RiseRandom,
) -> Result<RiseScoreView, RiseScoreError> {
    let b50 = compute_b50_from_records(scores, RatingMode::Actual)?;
    let seeds = candidates(snapshot, level)?;
    let best = best_by_chart(&scores.records);
    let profile = best
        .values()
        .map(|record| (**record).clone())
        .collect::<Vec<_>>();
    let legacy = Section {
        kind: RiseScoreSection::LegacyB35,
        records: &b50.b35,
        other: &b50.b15,
        capacity: 35,
    };
    let current = Section {
        kind: RiseScoreSection::CurrentB15,
        records: &b50.b15,
        other: &b50.b35,
        capacity: 15,
    };
    let (legacy_items, legacy_floor) =
        select_section(&legacy, &seeds, &best, &profile, score, algorithm, rng)?;
    let (current_items, current_floor) =
        select_section(&current, &seeds, &best, &profile, score, algorithm, rng)?;
    if legacy_items.is_empty() && current_items.is_empty() {
        return Err(RiseScoreError::NoRecommendations);
    }
    Ok(RiseScoreView {
        legacy: legacy_items,
        legacy_replacement_floor: legacy_floor,
        current: current_items,
        current_replacement_floor: current_floor,
    })
}

fn select_section(
    section: &Section<'_>,
    seeds: &[CandidateSeed],
    old: &HashMap<ChartKey, &B50Chart>,
    profile: &[B50Chart],
    score_filter: Option<u32>,
    algorithm: RiseScoreAlgorithm,
    rng: &mut impl RiseRandom,
) -> Result<(Vec<RiseScoreCandidate>, u32), RiseScoreError> {
    let replacement = replacement_floor(section.records, section.capacity);
    let candidate_floor = candidate_floor(section.records, section.other, section.capacity);
    let recommendation_floor = replacement.max(candidate_floor);
    let effective_floor = effective_candidate_floor(replacement, recommendation_floor);
    let targets = targets()?;
    let abilities = target_abilities(profile, &targets, recommendation_floor);
    let ignored = ignored_charts(section.records);
    let mut legacy = Vec::new();
    let mut expected_picks = Vec::new();
    for seed in seeds.iter().filter(|seed| {
        seed.is_current == (section.kind == RiseScoreSection::CurrentB15)
            && !ignored.contains(&seed.key)
    }) {
        let old_record = old.get(&seed.key).copied();
        let old_achievement = old_record.and_then(|record| {
            record
                .achievements
                .and_then(|achievement| achievement.ranked())
        });
        let old_rating = old_achievement
            .map(|achievement| single_song_rating(seed.constant, achievement))
            .transpose()?
            .unwrap_or(0);
        let bucket = fit_bucket(seed);
        match algorithm {
            RiseScoreAlgorithm::Legacy => {
                if let Some(item) = first_legacy(
                    seed,
                    old_achievement,
                    old_rating,
                    replacement,
                    recommendation_floor,
                    score_filter,
                    &targets,
                )? {
                    legacy.push((bucket, item));
                }
            }
            RiseScoreAlgorithm::Expected => {
                if let Some(item) = best_expected(
                    seed,
                    old_achievement,
                    old_rating,
                    replacement,
                    recommendation_floor,
                    effective_floor,
                    score_filter,
                    &targets,
                    &abilities,
                    bucket,
                )? {
                    expected_picks.push(item);
                }
            }
        }
    }
    let selected = match algorithm {
        RiseScoreAlgorithm::Legacy => select_legacy(legacy, rng),
        RiseScoreAlgorithm::Expected => select_expected(expected_picks, rng),
    };
    Ok((selected, replacement))
}

fn first_legacy(
    seed: &CandidateSeed,
    old_achievement: Option<AchievementRate>,
    old_rating: u32,
    replacement: u32,
    recommendation_floor: u32,
    score_filter: Option<u32>,
    targets: &[AchievementRate; 4],
) -> Result<Option<RiseScoreCandidate>, RiseScoreError> {
    for target in targets {
        let rating = single_song_rating(seed.constant, *target)?;
        if passes_floor(
            rating,
            old_rating,
            recommendation_floor,
            recommendation_floor,
            score_filter,
        ) {
            return Ok(Some(item(
                seed,
                old_achievement,
                old_rating,
                *target,
                rating,
                replacement,
            )));
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn best_expected(
    seed: &CandidateSeed,
    old_achievement: Option<AchievementRate>,
    old_rating: u32,
    replacement: u32,
    recommendation_floor: u32,
    effective_floor: u32,
    score_filter: Option<u32>,
    targets: &[AchievementRate; 4],
    abilities: &[(AchievementRate, Decimal); 4],
    bucket: u8,
) -> Result<Option<ExpectedPick>, RiseScoreError> {
    let mut best: Option<ExpectedPick> = None;
    for target in targets {
        let rating = single_song_rating(seed.constant, *target)?;
        if !passes_floor(
            rating,
            old_rating,
            effective_floor,
            recommendation_floor,
            score_filter,
        ) || !target_allowed(*target, old_achievement)
        {
            continue;
        }
        let ability = abilities
            .iter()
            .find(|(candidate, _)| candidate == target)
            .map(|(_, ability)| *ability)
            .unwrap_or(Decimal::ZERO);
        if seed.constant.value() > ability + margin_limit(*target, old_achievement) {
            continue;
        }
        let baseline = old_rating.max(replacement);
        let Some(actual_gain) = rating.checked_sub(baseline) else {
            continue;
        };
        let Some(over_floor) = rating.checked_sub(effective_floor) else {
            continue;
        };
        if actual_gain == 0 || over_floor == 0 {
            continue;
        }
        let score = candidate_score(ExpectedScoreInput {
            actual_gain,
            over_floor,
            replacement_floor: replacement,
            recommendation_floor,
            constant: seed.constant,
            ability,
            probability: probability(seed.constant, *target, ability, old_achievement),
            fit_bucket: bucket,
        });
        let candidate = ExpectedPick {
            score,
            gain: actual_gain,
            bucket,
            item: item(
                seed,
                old_achievement,
                old_rating,
                *target,
                rating,
                replacement,
            ),
        };
        if best
            .as_ref()
            .is_none_or(|current| expected_better(&candidate, current))
        {
            best = Some(candidate);
        }
    }
    Ok(best)
}

pub(super) fn passes_floor(
    rating: u32,
    old_rating: u32,
    gate_floor: u32,
    recommendation_floor: u32,
    score: Option<u32>,
) -> bool {
    rating > gate_floor
        && rating > old_rating
        && score
            .filter(|delta| *delta > 0)
            .is_none_or(|delta| rating >= recommendation_floor.saturating_add(delta))
}

fn item(
    seed: &CandidateSeed,
    old_achievement: Option<AchievementRate>,
    old_rating: u32,
    target: AchievementRate,
    target_rating: u32,
    replacement: u32,
) -> RiseScoreCandidate {
    RiseScoreCandidate {
        key: seed.key.clone(),
        display_id: seed.display_id.clone(),
        cover_id: seed.cover_id.clone(),
        image_name: seed.image_name.clone(),
        title: seed.title.clone(),
        constant: seed.constant,
        old_achievement,
        old_rating,
        target_achievement: target,
        target_rating,
        gain: target_rating.saturating_sub(old_rating.max(replacement)),
    }
}

pub(super) fn replacement_floor(records: &[B50Chart], capacity: usize) -> u32 {
    if records.len() >= capacity {
        records.last().and_then(|record| record.rating).unwrap_or(0)
    } else {
        0
    }
}

pub(super) fn candidate_floor(records: &[B50Chart], other: &[B50Chart], capacity: usize) -> u32 {
    let own = records.last().and_then(|record| record.rating).unwrap_or(0);
    let other = other.last().and_then(|record| record.rating).unwrap_or(0);
    if records.len() >= capacity {
        if own != 0 {
            own
        } else if other != 0 {
            other
        } else {
            250
        }
    } else {
        own.max(other).max(250)
    }
}

pub(super) fn effective_candidate_floor(replacement: u32, recommendation: u32) -> u32 {
    if replacement == 0 {
        recommendation.saturating_sub(18)
    } else {
        recommendation
    }
}

pub(super) fn ignored_charts(records: &[B50Chart]) -> HashSet<ChartKey> {
    records
        .iter()
        .filter(|record| {
            record
                .achievements
                .and_then(|value| value.ranked())
                .is_some_and(|value| value.ten_thousandths() >= 1_005_000)
        })
        .map(|record| record.key.clone())
        .collect()
}

pub(super) fn targets() -> Result<[AchievementRate; 4], maimai_core::RatingError> {
    Ok([
        AchievementRate::from_ten_thousandths(990_000)?,
        AchievementRate::from_ten_thousandths(995_000)?,
        AchievementRate::from_ten_thousandths(1_000_000)?,
        AchievementRate::from_ten_thousandths(1_005_000)?,
    ])
}
