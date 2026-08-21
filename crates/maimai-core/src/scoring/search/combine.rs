use indexmap::IndexMap;

use super::{Bucket, FlatCounts, add_bucket};
use crate::scoring::{
    ExactCount, Judgment, JudgmentCounts, JudgmentSet, NoteType, PerTypeSummary, ScoringError,
};

pub(super) fn combine_per_type_buckets(
    mut per_type_scores: Vec<(NoteType, IndexMap<i128, Bucket<FlatCounts>>)>,
    min_metric: i128,
    max_metric: i128,
    max_states: usize,
    sample_limit: usize,
) -> Result<IndexMap<i128, Bucket<JudgmentCounts>>, ScoringError> {
    per_type_scores.sort_by_key(|(note_type, scores)| (scores.len(), note_type.index()));
    let mut suffix_min = vec![0_i128; per_type_scores.len() + 1];
    let mut suffix_max = vec![0_i128; per_type_scores.len() + 1];
    for index in (0..per_type_scores.len()).rev() {
        let scores = &per_type_scores[index].1;
        let current_min = scores.keys().copied().min().unwrap_or(0);
        let current_max = scores.keys().copied().max().unwrap_or(0);
        suffix_min[index] = suffix_min[index + 1]
            .checked_add(current_min)
            .ok_or(ScoringError::Arithmetic("suffix minimum overflow"))?;
        suffix_max[index] = suffix_max[index + 1]
            .checked_add(current_max)
            .ok_or(ScoringError::Arithmetic("suffix maximum overflow"))?;
    }

    let initial_samples = if sample_limit == 0 {
        Vec::new()
    } else {
        vec![JudgmentCounts::empty()]
    };
    let mut states = IndexMap::from([(
        0_i128,
        Bucket {
            combination_count: ExactCount::one(),
            samples: initial_samples,
        },
    )]);
    for (index, (note_type, score_map)) in per_type_scores.into_iter().enumerate() {
        let lower_needed = min_metric
            .checked_sub(suffix_max[index + 1])
            .ok_or(ScoringError::Arithmetic("metric lower bound overflow"))?;
        let upper_allowed = max_metric
            .checked_sub(suffix_min[index + 1])
            .ok_or(ScoringError::Arithmetic("metric upper bound overflow"))?;
        let mut next_states = IndexMap::new();
        for (previous_metric, previous_bucket) in &states {
            for (metric, bucket) in &score_map {
                let new_metric = previous_metric
                    .checked_add(*metric)
                    .ok_or(ScoringError::Arithmetic("combined metric overflow"))?;
                if new_metric < lower_needed || new_metric > upper_allowed {
                    continue;
                }
                let samples = cross_nested_samples(
                    &previous_bucket.samples,
                    &bucket.samples,
                    note_type,
                    sample_limit,
                )?;
                let combinations = previous_bucket
                    .combination_count
                    .multiplied(&bucket.combination_count);
                add_bucket(
                    &mut next_states,
                    new_metric,
                    combinations,
                    samples,
                    sample_limit,
                );
            }
        }
        if next_states.len() > max_states {
            return Err(ScoringError::StateLimit {
                note_type: None,
                max_states,
            });
        }
        states = next_states;
    }
    states.retain(|metric, _| *metric >= min_metric && *metric <= max_metric);
    Ok(states)
}

fn cross_nested_samples(
    left: &[JudgmentCounts],
    right: &[FlatCounts],
    note_type: NoteType,
    sample_limit: usize,
) -> Result<Vec<JudgmentCounts>, ScoringError> {
    if sample_limit == 0 || left.is_empty() || right.is_empty() {
        return Ok(Vec::new());
    }
    let mut samples = Vec::new();
    'outer: for left_sample in left {
        for right_sample in right {
            let mut merged = left_sample.clone();
            for judgment in Judgment::ALL {
                merged.add(note_type, judgment, right_sample[judgment.index()])?;
            }
            samples.push(merged);
            if samples.len() >= sample_limit {
                break 'outer;
            }
        }
    }
    Ok(samples)
}

pub(super) fn summarize_type(
    note_type: NoteType,
    score_map: &IndexMap<i128, Bucket<FlatCounts>>,
    allowed: JudgmentSet,
) -> PerTypeSummary {
    let mut combination_count = ExactCount::zero();
    let mut sample_combination_count = 0_usize;
    for bucket in score_map.values() {
        combination_count.add_assign(&bucket.combination_count);
        sample_combination_count = sample_combination_count.saturating_add(bucket.samples.len());
    }
    PerTypeSummary {
        note_type,
        possible_metric_count: score_map.len(),
        combination_count,
        sample_combination_count,
        min_possible_metric: score_map.keys().copied().min().unwrap_or(0),
        max_possible_metric: score_map.keys().copied().max().unwrap_or(0),
        allowed_judgments: displayed_judgments(note_type, allowed),
    }
}

fn displayed_judgments(note_type: NoteType, judgments: JudgmentSet) -> Vec<String> {
    let mut values: Vec<String> = judgments
        .iter()
        .map(|judgment| judgment.display_name(note_type).to_owned())
        .collect();
    values.sort();
    values.dedup();
    values
}
