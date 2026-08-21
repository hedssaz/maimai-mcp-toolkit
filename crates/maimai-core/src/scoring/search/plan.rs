use super::{FlatCounts, FlatMetricValues};
use crate::scoring::calculate::score_value;
use crate::scoring::{
    Judgment, JudgmentSet, NoteType, OptionalJudgmentCounts, ScoreMode, ScoringError,
    SearchRequest, ShortcutConstraints,
};

#[derive(Clone, Debug)]
pub(super) struct NoteTypePlan {
    pub(super) note_type: NoteType,
    pub(super) judgments: Vec<Judgment>,
    pub(super) base_counts: FlatCounts,
    pub(super) remaining: u32,
    pub(super) max_remaining: FlatCounts,
    pub(super) fixed_metric: i128,
}

pub(super) fn normalized_constraints(
    request: &SearchRequest,
) -> Result<
    (
        [JudgmentSet; crate::scoring::types::NOTE_TYPE_COUNT],
        OptionalJudgmentCounts,
        ShortcutConstraints,
    ),
    ScoringError,
> {
    let mut allowed = request.constraints.allowed;
    for note_type in NoteType::ALL {
        allowed[note_type.index()] = allowed[note_type.index()]
            .difference(request.constraints.disallowed[note_type.index()]);
        if request.constraints.no_miss_good {
            allowed[note_type.index()].remove(Judgment::Miss);
            allowed[note_type.index()].remove(Judgment::Good);
        }
    }

    let mut minimums = request.constraints.minimum.clone();
    let mut required_critical = None;
    if let Some(cap) = request.constraints.break_max_non_critical {
        let required = request.note_totals.get(NoteType::Break).saturating_sub(cap);
        if required != 0 {
            let existing = minimums
                .get(NoteType::Break, Judgment::Critical)
                .unwrap_or(0);
            minimums.set(
                NoteType::Break,
                Judgment::Critical,
                Some(existing.max(required)),
            );
        }
        required_critical = Some(required);
    }

    Ok((
        allowed,
        minimums,
        ShortcutConstraints {
            no_miss_good: request.constraints.no_miss_good,
            break_max_perfect_or_below: request.constraints.break_max_non_critical,
            break_min_critical: required_critical,
        },
    ))
}

pub(super) fn build_note_type_plan(
    request: &SearchRequest,
    note_type: NoteType,
    score_mode: ScoreMode,
    allowed: JudgmentSet,
    minimums: &OptionalJudgmentCounts,
) -> Result<NoteTypePlan, ScoringError> {
    let total = request.note_totals.get(note_type);
    if total == 0 {
        return Ok(NoteTypePlan {
            note_type,
            judgments: Vec::new(),
            base_counts: [0; crate::scoring::types::JUDGMENT_COUNT],
            remaining: 0,
            max_remaining: [0; crate::scoring::types::JUDGMENT_COUNT],
            fixed_metric: 0,
        });
    }

    let mut base_counts = [0_u32; crate::scoring::types::JUDGMENT_COUNT];
    let mut exact = JudgmentSet::EMPTY;
    for judgment in Judgment::ALL {
        let fixed = request.constraints.fixed.get(note_type, judgment);
        let minimum = minimums.get(note_type, judgment);
        let maximum = request.constraints.maximum.get(note_type, judgment);
        if (fixed.is_some() || minimum.is_some() || maximum.is_some())
            && !allowed.contains(judgment)
        {
            return Err(constraint_error(
                note_type,
                judgment,
                "is constrained but not allowed",
            ));
        }
        if let Some(fixed_count) = fixed {
            if minimum.is_some_and(|value| fixed_count < value) {
                return Err(constraint_error(
                    note_type,
                    judgment,
                    "fixed count is below its minimum",
                ));
            }
            if maximum.is_some_and(|value| fixed_count > value) {
                return Err(constraint_error(
                    note_type,
                    judgment,
                    "fixed count is above its maximum",
                ));
            }
            base_counts[judgment.index()] = fixed_count;
            exact.insert(judgment);
        }
    }
    for judgment in Judgment::ALL {
        if exact.contains(judgment) {
            continue;
        }
        let minimum = minimums.get(note_type, judgment);
        let maximum = request.constraints.maximum.get(note_type, judgment);
        if let Some(minimum_count) = minimum {
            if maximum.is_some_and(|value| minimum_count > value) {
                return Err(constraint_error(
                    note_type,
                    judgment,
                    "minimum is above maximum",
                ));
            }
            base_counts[judgment.index()] = minimum_count;
        }
    }

    let used = base_counts.iter().try_fold(0_u64, |sum, value| {
        sum.checked_add(u64::from(*value))
            .ok_or(ScoringError::Arithmetic("constraint count sum overflow"))
    })?;
    if used > u64::from(total) {
        return Err(ScoringError::Constraint(format!(
            "{note_type} constraints use {used} notes, above total {total}"
        )));
    }
    let remaining = total - used as u32;
    let judgments: Vec<_> = Judgment::ALL
        .into_iter()
        .filter(|judgment| allowed.contains(*judgment))
        .collect();
    if judgments.is_empty() && remaining != 0 {
        return Err(ScoringError::Constraint(format!(
            "{note_type} has notes left but no allowed judgments"
        )));
    }

    let mut max_remaining = [0_u32; crate::scoring::types::JUDGMENT_COUNT];
    for judgment in &judgments {
        let cap = if exact.contains(*judgment) {
            0
        } else if let Some(maximum) = request.constraints.maximum.get(note_type, *judgment) {
            maximum
                .checked_sub(base_counts[judgment.index()])
                .ok_or_else(|| {
                    constraint_error(note_type, *judgment, "maximum is below required count")
                })?
        } else {
            remaining
        };
        max_remaining[judgment.index()] = cap;
    }
    let capacity = max_remaining.iter().try_fold(0_u64, |sum, value| {
        sum.checked_add(u64::from(*value))
            .ok_or(ScoringError::Arithmetic("constraint capacity overflow"))
    })?;
    if capacity < u64::from(remaining) {
        return Err(ScoringError::Constraint(format!(
            "{note_type} max constraints cannot fill {remaining} remaining notes"
        )));
    }
    let values = metric_values_for_type(note_type, score_mode);
    let fixed_metric = weighted_sum(&base_counts, &values)?;
    Ok(NoteTypePlan {
        note_type,
        judgments,
        base_counts,
        remaining,
        max_remaining,
        fixed_metric,
    })
}

pub(super) fn metric_values_for_type(note_type: NoteType, mode: ScoreMode) -> FlatMetricValues {
    let mut values = [0_i128; crate::scoring::types::JUDGMENT_COUNT];
    for judgment in Judgment::ALL {
        values[judgment.index()] = score_value(note_type, judgment, mode);
    }
    values
}

pub(super) fn weighted_sum(
    counts: &FlatCounts,
    values: &FlatMetricValues,
) -> Result<i128, ScoringError> {
    counts
        .iter()
        .zip(values)
        .try_fold(0_i128, |sum, (count, value)| {
            value
                .checked_mul(i128::from(*count))
                .and_then(|contribution| sum.checked_add(contribution))
                .ok_or(ScoringError::Arithmetic("weighted metric sum overflow"))
        })
}

fn constraint_error(note_type: NoteType, judgment: Judgment, detail: &str) -> ScoringError {
    ScoringError::Constraint(format!("{note_type}.{judgment} {detail}"))
}
