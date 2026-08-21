mod combine;
mod plan;
mod solver;
mod target;

use std::hash::Hash;

use indexmap::IndexMap;

use super::calculate::{percentage_details, validate_display_digits};
use super::rational::Rational;
use super::{
    ExactCount, MetricDetails, NoteType, ScoreCountRequest, ScoreMode, ScoringError, SearchRequest,
    SearchResult, SearchSolution, ShortcutConstraints, TargetRange, score_counts,
};
use combine::{combine_per_type_buckets, summarize_type};
use plan::{build_note_type_plan, metric_values_for_type, normalized_constraints, weighted_sum};
use solver::possible_metrics_for_plan;
use target::{achievement_loss_range, achievement_metric, raw_target_range};

type FlatCounts = [u32; super::types::JUDGMENT_COUNT];
type FlatMetricValues = [i128; super::types::JUDGMENT_COUNT];

#[derive(Clone, Debug)]
struct Bucket<S> {
    combination_count: ExactCount,
    samples: Vec<S>,
}

impl<S> Bucket<S> {
    fn empty() -> Self {
        Self {
            combination_count: ExactCount::zero(),
            samples: Vec::new(),
        }
    }
}

fn add_bucket<K: Eq + Hash, S>(
    buckets: &mut IndexMap<K, Bucket<S>>,
    key: K,
    combination_count: ExactCount,
    samples: Vec<S>,
    sample_limit: usize,
) {
    let bucket = buckets.entry(key).or_insert_with(Bucket::empty);
    bucket.combination_count.add_assign(&combination_count);
    let remaining = sample_limit.saturating_sub(bucket.samples.len());
    bucket.samples.extend(samples.into_iter().take(remaining));
}

/// Find judgment-count vectors that produce the requested raw score or displayed
/// achievement percentage.
///
/// `matching_combination_count` counts count vectors rather than permutations of
/// individual notes. It remains exact even when `max_solutions` is zero or only a
/// small preview of those vectors is returned.
pub fn find_score_combinations(request: &SearchRequest) -> Result<SearchResult, ScoringError> {
    if request.note_totals.total() == 0 {
        return Err(ScoringError::InvalidInput(
            "note_totals must contain at least one note".to_owned(),
        ));
    }
    if request.max_states == 0 {
        return Err(ScoringError::InvalidInput(
            "max_states must be greater than zero".to_owned(),
        ));
    }

    match request.score_mode {
        ScoreMode::OldAchievement | ScoreMode::DxAchievement => find_achievement(request),
        _ => find_raw(request),
    }
}

fn find_raw(request: &SearchRequest) -> Result<SearchResult, ScoringError> {
    let (min_score, max_score) = raw_target_range(&request.target)?;
    let (allowed, minimums, shortcuts) = normalized_constraints(request)?;
    let mut per_type_scores = Vec::with_capacity(super::types::NOTE_TYPE_COUNT);
    let mut per_type_summary = Vec::new();

    for note_type in NoteType::ALL {
        let plan = build_note_type_plan(
            request,
            note_type,
            request.score_mode,
            allowed[note_type.index()],
            &minimums,
        )?;
        let score_map = possible_metrics_for_plan(
            &plan,
            request.max_states,
            request.max_solutions,
            &metric_values_for_type(note_type, request.score_mode),
            Some(max_score),
        )?;
        if request.note_totals.get(note_type) != 0 && score_map.is_empty() {
            return Err(ScoringError::Constraint(format!(
                "{note_type} has no possible scores under the constraints"
            )));
        }
        if !score_map.is_empty() && request.note_totals.get(note_type) != 0 {
            per_type_summary.push(summarize_type(
                note_type,
                &score_map,
                allowed[note_type.index()],
            ));
        }
        per_type_scores.push((note_type, score_map));
    }

    let states = combine_per_type_buckets(
        per_type_scores,
        min_score,
        max_score,
        request.max_states,
        request.max_solutions,
    )?;
    let mut matching_combination_count = ExactCount::zero();
    let mut solutions = Vec::new();
    let mut candidate_scores = states.keys().copied().collect::<Vec<_>>();
    candidate_scores.sort_unstable();
    for score in candidate_scores {
        let bucket = states.get(&score).ok_or(ScoringError::Arithmetic(
            "candidate score disappeared during search",
        ))?;
        matching_combination_count.add_assign(&bucket.combination_count);
        for counts in &bucket.samples {
            if solutions.len() >= request.max_solutions {
                break;
            }
            let totals = score_counts(&ScoreCountRequest {
                counts: counts.clone(),
                score_mode: Some(request.score_mode),
                display_digits: 4,
                include_zero: false,
            })?
            .totals;
            solutions.push(SearchSolution {
                metric: score,
                loss_metric: None,
                percentage: None,
                counts: counts.clone(),
                totals,
            });
        }
    }

    let returned_solution_count = solutions.len();
    let truncated = ExactCount::from(returned_solution_count as u64) < matching_combination_count;
    Ok(SearchResult {
        found: !states.is_empty(),
        score_mode: request.score_mode,
        display_mode: None,
        display_digits: None,
        note_totals: request.note_totals.clone(),
        target_range: TargetRange::Raw {
            min_score,
            max_score,
        },
        metric_details: None,
        matching_metric_count: states.len(),
        matching_combination_count,
        returned_solution_count,
        truncated,
        solutions,
        per_type_summary,
        shortcut_constraints: shortcuts,
    })
}

fn find_achievement(request: &SearchRequest) -> Result<SearchResult, ScoringError> {
    validate_display_digits(request.display_digits)?;
    let metric = achievement_metric(request.score_mode, &request.note_totals)?;
    let loss_range = achievement_loss_range(request, &metric)?;
    let mut metric_details = metric.details.clone();
    metric_details.min_loss = loss_range.min_loss;
    metric_details.max_loss = loss_range.max_loss;

    if loss_range.max_loss < loss_range.min_loss {
        return Ok(empty_achievement_result(
            request,
            loss_range.target_range,
            metric_details,
        ));
    }

    let (allowed, minimums, shortcuts) = normalized_constraints(request)?;
    let mut per_type_scores = Vec::with_capacity(super::types::NOTE_TYPE_COUNT);
    let mut per_type_summary = Vec::new();
    for note_type in NoteType::ALL {
        let mut plan = build_note_type_plan(
            request,
            note_type,
            ScoreMode::OldScore,
            allowed[note_type.index()],
            &minimums,
        )?;
        let loss_values = metric.loss_values[note_type.index()];
        plan.fixed_metric = weighted_sum(&plan.base_counts, &loss_values)?;
        let score_map = possible_metrics_for_plan(
            &plan,
            request.max_states,
            request.max_solutions,
            &loss_values,
            Some(loss_range.max_loss),
        )?;
        if request.note_totals.get(note_type) != 0 && score_map.is_empty() {
            return Err(ScoringError::Constraint(format!(
                "{note_type} has no possible {} losses under the constraints",
                request.score_mode.as_str()
            )));
        }
        if !score_map.is_empty() && request.note_totals.get(note_type) != 0 {
            per_type_summary.push(summarize_type(
                note_type,
                &score_map,
                allowed[note_type.index()],
            ));
        }
        per_type_scores.push((note_type, score_map));
    }

    let states = combine_per_type_buckets(
        per_type_scores,
        loss_range.min_loss,
        loss_range.max_loss,
        request.max_states,
        request.max_solutions,
    )?;
    let mut matching_combination_count = ExactCount::zero();
    let mut solutions = Vec::new();
    let mut candidate_losses = states.keys().copied().collect::<Vec<_>>();
    candidate_losses.sort_unstable();
    for loss in candidate_losses {
        let bucket = states.get(&loss).ok_or(ScoringError::Arithmetic(
            "candidate loss disappeared during search",
        ))?;
        matching_combination_count.add_assign(&bucket.combination_count);
        for counts in &bucket.samples {
            if solutions.len() >= request.max_solutions {
                break;
            }
            let totals = score_counts(&ScoreCountRequest {
                counts: counts.clone(),
                score_mode: Some(request.score_mode),
                display_digits: request.display_digits,
                include_zero: false,
            })?
            .totals;
            let earned_metric =
                metric
                    .max_metric
                    .checked_sub(loss)
                    .ok_or(ScoringError::Arithmetic(
                        "achievement metric subtraction overflow",
                    ))?;
            let percentage = percentage_details(
                Rational::new(earned_metric, metric.denominator)?,
                request.display_digits,
            )?;
            solutions.push(SearchSolution {
                metric: earned_metric,
                loss_metric: Some(loss),
                percentage: Some(percentage),
                counts: counts.clone(),
                totals,
            });
        }
    }

    let returned_solution_count = solutions.len();
    let truncated = ExactCount::from(returned_solution_count as u64) < matching_combination_count;
    Ok(SearchResult {
        found: !states.is_empty(),
        score_mode: request.score_mode,
        display_mode: Some(request.display_mode),
        display_digits: Some(request.display_digits),
        note_totals: request.note_totals.clone(),
        target_range: loss_range.target_range,
        metric_details: Some(metric_details),
        matching_metric_count: states.len(),
        matching_combination_count,
        returned_solution_count,
        truncated,
        solutions,
        per_type_summary,
        shortcut_constraints: shortcuts,
    })
}

fn empty_achievement_result(
    request: &SearchRequest,
    target_range: TargetRange,
    metric_details: MetricDetails,
) -> SearchResult {
    SearchResult {
        found: false,
        score_mode: request.score_mode,
        display_mode: Some(request.display_mode),
        display_digits: Some(request.display_digits),
        note_totals: request.note_totals.clone(),
        target_range,
        metric_details: Some(metric_details),
        matching_metric_count: 0,
        matching_combination_count: ExactCount::zero(),
        returned_solution_count: 0,
        truncated: false,
        solutions: Vec::new(),
        per_type_summary: Vec::new(),
        shortcut_constraints: ShortcutConstraints {
            no_miss_good: request.constraints.no_miss_good,
            break_max_perfect_or_below: request.constraints.break_max_non_critical,
            break_min_critical: request
                .constraints
                .break_max_non_critical
                .map(|cap| request.note_totals.get(NoteType::Break).saturating_sub(cap)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::find_score_combinations;
    use crate::scoring::{
        DisplayMode, ExactCount, Judgment, JudgmentSet, NoteTotals, NoteType, PercentageInput,
        ScoreMode, ScoringError, SearchRequest, SearchTarget,
    };

    #[test]
    fn exact_count_survives_limited_samples() -> Result<(), ScoringError> {
        let mut request = SearchRequest::raw(
            NoteTotals::new(0, 0, 0, 0, 2),
            ScoreMode::OldScore,
            SearchTarget::RawExact(5_100),
        );
        request.max_solutions = 1;
        let result = find_score_combinations(&request)?;
        assert!(result.found);
        assert_eq!(result.matching_metric_count, 1);
        assert_eq!(result.matching_combination_count, ExactCount::from(2));
        assert_eq!(result.returned_solution_count, 1);
        assert!(result.truncated);
        assert_eq!(
            result.solutions[0]
                .counts
                .get(NoteType::Break, Judgment::PerfectHigh),
            2
        );
        assert_eq!(
            result.solutions[0]
                .counts
                .get(NoteType::Break, Judgment::PerfectLow),
            0
        );
        assert_eq!(
            result.solutions[0]
                .counts
                .get(NoteType::Break, Judgment::Critical),
            0
        );
        Ok(())
    }

    #[test]
    fn zero_solution_limit_keeps_exact_count() -> Result<(), ScoringError> {
        let mut request = SearchRequest::raw(
            NoteTotals::new(0, 0, 0, 0, 2),
            ScoreMode::OldScore,
            SearchTarget::RawExact(5_100),
        );
        request.max_solutions = 0;
        let result = find_score_combinations(&request)?;
        assert!(result.found);
        assert_eq!(result.matching_combination_count, ExactCount::from(2));
        assert!(result.solutions.is_empty());
        assert!(result.truncated);
        Ok(())
    }

    #[test]
    fn dxacc_uses_loss_window_from_perfect_score() -> Result<(), ScoringError> {
        let mut request = SearchRequest::raw(
            NoteTotals::new(2, 0, 0, 0, 1),
            ScoreMode::DxAchievement,
            SearchTarget::PercentageExact(PercentageInput::Decimal("100.5000%".to_owned())),
        );
        request.display_mode = DisplayMode::Floor;
        request.max_solutions = 0;
        let result = find_score_combinations(&request)?;
        assert!(result.found);
        assert!(result.matching_combination_count >= ExactCount::one());
        let metric = result.metric_details.ok_or_else(|| {
            ScoringError::InvalidInput("missing metric details in test".to_owned())
        })?;
        assert!(metric.min_loss >= 0);
        assert!(metric.max_loss >= metric.min_loss);
        Ok(())
    }

    #[test]
    fn shortcut_and_underlying_judgment_vectors_are_preserved() -> Result<(), ScoringError> {
        let mut request = SearchRequest::raw(
            NoteTotals::new(3, 0, 0, 0, 0),
            ScoreMode::DxScore,
            SearchTarget::RawExact(3),
        );
        let mut allowed = JudgmentSet::EMPTY;
        allowed.insert(Judgment::GreatLow);
        allowed.insert(Judgment::GreatMid);
        allowed.insert(Judgment::GreatHigh);
        allowed.insert(Judgment::PerfectLow);
        allowed.insert(Judgment::PerfectHigh);
        request.constraints.allow_only(NoteType::Tap, allowed);
        request.max_solutions = 1;
        let result = find_score_combinations(&request)?;
        assert_eq!(result.matching_combination_count, ExactCount::from(10));
        assert_eq!(result.returned_solution_count, 1);
        assert!(result.truncated);
        Ok(())
    }
}
