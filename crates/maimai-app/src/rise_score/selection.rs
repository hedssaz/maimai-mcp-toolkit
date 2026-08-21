use maimai_render::RiseScoreCandidate;
use rust_decimal::Decimal;

use super::{catalog::CandidateSeed, rng::RiseRandom};

const LIMIT: usize = 5;

#[derive(Clone)]
pub(super) struct ExpectedPick {
    pub score: Decimal,
    pub gain: u32,
    pub bucket: u8,
    pub item: RiseScoreCandidate,
}

pub(super) fn expected_better(left: &ExpectedPick, right: &ExpectedPick) -> bool {
    (
        left.score,
        left.gain,
        std::cmp::Reverse(left.bucket),
        left.item.target_rating,
    ) > (
        right.score,
        right.gain,
        std::cmp::Reverse(right.bucket),
        right.item.target_rating,
    )
}

pub(super) fn fit_bucket(seed: &CandidateSeed) -> u8 {
    let Some(delta) = seed.fit_diff.map(|fit| seed.constant.value() - fit) else {
        return 4;
    };
    if delta > Decimal::new(2, 1) {
        0
    } else if delta >= Decimal::ZERO {
        1
    } else if delta >= Decimal::new(-2, 1) {
        2
    } else {
        3
    }
}

pub(super) fn select_legacy(
    candidates: Vec<(u8, RiseScoreCandidate)>,
    rng: &mut impl RiseRandom,
) -> Vec<RiseScoreCandidate> {
    let mut selected = Vec::new();
    for bucket in 0..=4 {
        let mut items = candidates
            .iter()
            .filter(|(candidate_bucket, _)| *candidate_bucket == bucket)
            .map(|(_, item)| item.clone())
            .collect::<Vec<_>>();
        let remaining = LIMIT - selected.len();
        if items.len() <= remaining {
            selected.extend(items);
        } else {
            while selected.len() < LIMIT {
                let index = rng.below(items.len());
                selected.push(items.swap_remove(index));
            }
        }
        if selected.len() == LIMIT {
            break;
        }
    }
    sort_display(&mut selected);
    selected
}

pub(super) fn select_expected(
    mut candidates: Vec<ExpectedPick>,
    rng: &mut impl RiseRandom,
) -> Vec<RiseScoreCandidate> {
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.gain.cmp(&left.gain))
            .then_with(|| left.bucket.cmp(&right.bucket))
            .then_with(|| right.item.target_rating.cmp(&left.item.target_rating))
            .then_with(|| right.item.display_id.cmp(&left.item.display_id))
            .then_with(|| right.item.difficulty().cmp(&left.item.difficulty()))
    });
    if candidates.len() <= LIMIT {
        let mut selected = candidates
            .into_iter()
            .map(|candidate| candidate.item)
            .collect::<Vec<_>>();
        sort_display(&mut selected);
        return selected;
    }
    let pool_len = candidates.len().min((LIMIT * 8).max(30));
    candidates.truncate(pool_len);
    let pool_size = candidates.len();
    let mut candidates = candidates
        .into_iter()
        .enumerate()
        .map(|(rank, candidate)| (candidate, rank_weight(pool_size, rank)))
        .collect::<Vec<_>>();
    let mut selected = Vec::new();
    while !candidates.is_empty() && selected.len() < LIMIT {
        let weights = candidates
            .iter()
            .map(|(_, weight)| *weight)
            .collect::<Vec<_>>();
        let index = rng.weighted(&weights).min(candidates.len() - 1);
        selected.push(candidates.remove(index).0.item);
    }
    sort_display(&mut selected);
    selected
}

pub(super) fn rank_weight(pool_size: usize, rank: usize) -> u64 {
    let quality = (pool_size.saturating_sub(rank)) as f64 / pool_size.max(1) as f64;
    (((0.28 + quality.powf(1.15)) * 1_000_000.0).round() as u64).max(1)
}

fn sort_display(items: &mut [RiseScoreCandidate]) {
    items.sort_by(|left, right| {
        right
            .display_id
            .cmp(&left.display_id)
            .then_with(|| right.difficulty().cmp(&left.difficulty()))
    });
}
