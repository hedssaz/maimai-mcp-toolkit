use std::collections::BTreeMap;

use indexmap::IndexMap;

use super::plan::NoteTypePlan;
use super::{Bucket, FlatCounts, FlatMetricValues, add_bucket};
use crate::scoring::count::bounded_composition_count;
use crate::scoring::{ExactCount, Judgment, ScoringError};

pub(super) fn possible_metrics_for_plan(
    plan: &NoteTypePlan,
    max_states: usize,
    sample_limit: usize,
    values: &FlatMetricValues,
    metric_cap: Option<i128>,
) -> Result<IndexMap<i128, Bucket<FlatCounts>>, ScoringError> {
    let effective_cap = match metric_cap {
        Some(cap) => {
            let remaining_cap = cap
                .checked_sub(plan.fixed_metric)
                .ok_or(ScoringError::Arithmetic("metric cap subtraction overflow"))?;
            if remaining_cap < 0 {
                return Ok(IndexMap::new());
            }
            Some(remaining_cap)
        }
        None => None,
    };
    if plan.remaining == 0 {
        if metric_cap.is_some_and(|cap| plan.fixed_metric > cap) {
            return Ok(IndexMap::new());
        }
        let samples = if sample_limit == 0 {
            Vec::new()
        } else {
            vec![plan.base_counts]
        };
        return Ok(IndexMap::from([(
            plan.fixed_metric,
            Bucket {
                combination_count: ExactCount::one(),
                samples,
            },
        )]));
    }

    let mut zero_group = Vec::new();
    let mut grouped: BTreeMap<i128, Vec<Judgment>> = BTreeMap::new();
    for judgment in &plan.judgments {
        if plan.max_remaining[judgment.index()] == 0 {
            continue;
        }
        let value = values[judgment.index()];
        if value == 0 {
            zero_group.push(*judgment);
        } else {
            grouped.entry(value).or_default().push(*judgment);
        }
    }
    let zero_capacity = zero_group.iter().try_fold(0_u32, |sum, judgment| {
        sum.checked_add(plan.max_remaining[judgment.index()])
            .ok_or(ScoringError::Arithmetic("zero-value capacity overflow"))
    })?;

    let initial_samples = if sample_limit == 0 {
        Vec::new()
    } else {
        vec![plan.base_counts]
    };
    let mut states = IndexMap::from([(
        (0_u32, 0_i128),
        Bucket {
            combination_count: ExactCount::one(),
            samples: initial_samples,
        },
    )]);

    for (value, judgments) in grouped {
        let mut group_cap = judgments.iter().try_fold(0_u32, |sum, judgment| {
            sum.checked_add(plan.max_remaining[judgment.index()])
                .ok_or(ScoringError::Arithmetic("judgment group capacity overflow"))
        })?;
        group_cap = group_cap.min(plan.remaining);
        if let Some(cap) = effective_cap
            && value > 0
        {
            group_cap = group_cap.min(i128_to_u32_saturating(cap / value));
        }
        let distributions =
            distribution_buckets(&judgments, &plan.max_remaining, group_cap, sample_limit);
        let mut next_states = IndexMap::new();
        for ((used, score), bucket) in &states {
            let max_count = group_cap.min(plan.remaining - *used);
            for (count, distribution) in &distributions {
                if *count > max_count {
                    break;
                }
                let new_used = used
                    .checked_add(*count)
                    .ok_or(ScoringError::Arithmetic("used-note count overflow"))?;
                let added = value
                    .checked_mul(i128::from(*count))
                    .ok_or(ScoringError::Arithmetic("metric multiplication overflow"))?;
                let new_score = score
                    .checked_add(added)
                    .ok_or(ScoringError::Arithmetic("metric addition overflow"))?;
                if effective_cap.is_some_and(|cap| new_score > cap) {
                    continue;
                }
                let samples =
                    cross_flat_samples(&bucket.samples, &distribution.samples, sample_limit)?;
                let combinations = bucket
                    .combination_count
                    .multiplied(&distribution.combination_count);
                add_bucket(
                    &mut next_states,
                    (new_used, new_score),
                    combinations,
                    samples,
                    sample_limit,
                );
            }
        }
        if next_states.len() > max_states {
            return Err(ScoringError::StateLimit {
                note_type: Some(plan.note_type),
                max_states,
            });
        }
        states = next_states;
    }

    let zero_distributions = if zero_group.is_empty() {
        let samples = if sample_limit == 0 {
            Vec::new()
        } else {
            vec![[0; crate::scoring::types::JUDGMENT_COUNT]]
        };
        IndexMap::from([(
            0,
            Bucket {
                combination_count: ExactCount::one(),
                samples,
            },
        )])
    } else {
        distribution_buckets(
            &zero_group,
            &plan.max_remaining,
            plan.remaining,
            sample_limit,
        )
    };
    let mut results = IndexMap::new();
    for ((used, score), bucket) in states {
        let Some(zero_fill) = plan.remaining.checked_sub(used) else {
            continue;
        };
        if zero_group.is_empty() && used != plan.remaining {
            continue;
        }
        if !zero_group.is_empty() && zero_fill > zero_capacity {
            continue;
        }
        let Some(zero_distribution) = zero_distributions.get(&zero_fill) else {
            continue;
        };
        let total_metric = plan
            .fixed_metric
            .checked_add(score)
            .ok_or(ScoringError::Arithmetic("metric total overflow"))?;
        let samples =
            cross_flat_samples(&bucket.samples, &zero_distribution.samples, sample_limit)?;
        let combinations = bucket
            .combination_count
            .multiplied(&zero_distribution.combination_count);
        add_bucket(
            &mut results,
            total_metric,
            combinations,
            samples,
            sample_limit,
        );
    }
    Ok(results)
}

fn distribution_buckets(
    judgments: &[Judgment],
    max_remaining: &FlatCounts,
    max_total: u32,
    sample_limit: usize,
) -> IndexMap<u32, Bucket<FlatCounts>> {
    if judgments.is_empty() {
        let samples = if sample_limit == 0 {
            Vec::new()
        } else {
            vec![[0; crate::scoring::types::JUDGMENT_COUNT]]
        };
        return IndexMap::from([(
            0,
            Bucket {
                combination_count: ExactCount::one(),
                samples,
            },
        )]);
    }
    let caps: Vec<u32> = judgments
        .iter()
        .map(|judgment| max_remaining[judgment.index()].min(max_total))
        .collect();
    let capacity: u64 = caps.iter().copied().map(u64::from).sum();
    let hard_max = max_total.min(u32::try_from(capacity).unwrap_or(u32::MAX));
    let mut buckets = IndexMap::new();
    for total in 0..=hard_max {
        let combination_count = bounded_composition_count(total, &caps);
        if combination_count.is_zero() {
            continue;
        }
        buckets.insert(
            total,
            Bucket {
                combination_count,
                samples: bounded_distribution_samples(judgments, &caps, total, sample_limit),
            },
        );
    }
    buckets
}

fn bounded_distribution_samples(
    judgments: &[Judgment],
    caps: &[u32],
    total: u32,
    sample_limit: usize,
) -> Vec<FlatCounts> {
    if sample_limit == 0 {
        return Vec::new();
    }
    fn visit(
        index: usize,
        remaining: u32,
        judgments: &[Judgment],
        caps: &[u32],
        current: &mut FlatCounts,
        samples: &mut Vec<FlatCounts>,
        sample_limit: usize,
    ) {
        if samples.len() >= sample_limit {
            return;
        }
        if index == judgments.len() {
            if remaining == 0 {
                samples.push(*current);
            }
            return;
        }
        let judgment = judgments[index];
        for count in 0..=caps[index].min(remaining) {
            current[judgment.index()] = count;
            visit(
                index + 1,
                remaining - count,
                judgments,
                caps,
                current,
                samples,
                sample_limit,
            );
            if samples.len() >= sample_limit {
                break;
            }
        }
        current[judgment.index()] = 0;
    }

    let mut samples = Vec::new();
    let mut current = [0; crate::scoring::types::JUDGMENT_COUNT];
    visit(
        0,
        total,
        judgments,
        caps,
        &mut current,
        &mut samples,
        sample_limit,
    );
    samples
}

fn cross_flat_samples(
    left: &[FlatCounts],
    right: &[FlatCounts],
    sample_limit: usize,
) -> Result<Vec<FlatCounts>, ScoringError> {
    if sample_limit == 0 || left.is_empty() || right.is_empty() {
        return Ok(Vec::new());
    }
    let mut samples = Vec::new();
    'outer: for left_sample in left {
        for right_sample in right {
            let mut merged = *left_sample;
            for judgment in Judgment::ALL {
                merged[judgment.index()] = merged[judgment.index()]
                    .checked_add(right_sample[judgment.index()])
                    .ok_or(ScoringError::Arithmetic("sample count overflow"))?;
            }
            samples.push(merged);
            if samples.len() >= sample_limit {
                break 'outer;
            }
        }
    }
    Ok(samples)
}

fn i128_to_u32_saturating(value: i128) -> u32 {
    if value <= 0 {
        0
    } else if value >= i128::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}
