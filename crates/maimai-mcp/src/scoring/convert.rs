use std::str::FromStr;

use maimai_core::scoring::{
    DisplayMode, Judgment, JudgmentCounts, JudgmentGroup, JudgmentSet, NoteTotals, NoteType,
    OptionalJudgmentCounts, PercentageInput, ScoreCountRequest, ScoreMode, SearchConstraints,
    SearchRequest, SearchTarget,
};

use super::{
    dto::{FindCombinationsArgs, NestedCounts, OneOrMany, ScoreCountsArgs, ScoreValue},
    error::AdapterError,
};

const NOTE_TYPE_COUNT: usize = 5;

pub(super) fn score_count_request(
    arguments: ScoreCountsArgs,
) -> Result<ScoreCountRequest, AdapterError> {
    Ok(ScoreCountRequest {
        counts: judgment_counts(arguments.counts)?,
        score_mode: arguments
            .score_mode
            .as_deref()
            .map(ScoreMode::from_str)
            .transpose()?,
        display_digits: arguments.display_digits,
        include_zero: arguments.include_zero,
    })
}

pub(super) fn search_request(
    arguments: FindCombinationsArgs,
) -> Result<SearchRequest, AdapterError> {
    let score_mode = ScoreMode::from_str(&arguments.score_mode)?;
    let target = if score_mode.is_achievement() {
        achievement_target(&arguments)?
    } else {
        raw_target(&arguments)?
    };
    let note_totals = note_totals(arguments.note_totals)?;
    let mut constraints = SearchConstraints::default();
    apply_restrictions(&arguments.allowed_judgments, &mut constraints.allowed)?;
    apply_restrictions(&arguments.disallowed_judgments, &mut constraints.disallowed)?;
    constraints.fixed = optional_counts(arguments.fixed_counts)?;
    constraints.minimum = optional_counts(arguments.min_counts)?;
    constraints.maximum = optional_counts(arguments.max_counts)?;
    constraints.no_miss_good = agreeing_bool(
        "no_miss_good/all_notes_no_miss_good/fc_plus_only/no_good_miss aliases disagree",
        [
            arguments.no_miss_good,
            arguments.all_notes_no_miss_good,
            arguments.fc_plus_only,
            arguments.no_good_miss,
        ],
    )?
    .unwrap_or(false);
    constraints.break_max_non_critical = agreeing_u32(
        "Break non-critical cap aliases disagree",
        [
            arguments.break_max_perfect_or_below,
            arguments.break_max_perfect_or_lower,
            arguments.break_max_below_critical,
            arguments.max_break_below_critical,
            arguments.break_max_non_critical,
            arguments.max_break_non_critical,
        ],
    )?;

    Ok(SearchRequest {
        note_totals,
        score_mode,
        target,
        constraints,
        max_solutions: arguments.max_solutions,
        max_states: arguments.max_states,
        display_digits: arguments.display_digits,
        display_mode: display_mode(&arguments.display_mode)?,
    })
}

fn judgment_counts(raw: NestedCounts) -> Result<JudgmentCounts, AdapterError> {
    let mut result = JudgmentCounts::empty();
    for (raw_note_type, raw_counts) in raw {
        let note_type = NoteType::from_str(&raw_note_type)?;
        for (raw_judgment, count) in raw_counts {
            let judgment = Judgment::parse_count(&raw_judgment, note_type)?;
            result.add(note_type, judgment, count)?;
        }
    }
    Ok(result)
}

fn optional_counts(raw: NestedCounts) -> Result<OptionalJudgmentCounts, AdapterError> {
    let mut result = OptionalJudgmentCounts::empty();
    for (raw_note_type, raw_counts) in raw {
        let note_type = NoteType::from_str(&raw_note_type)?;
        for (raw_judgment, count) in raw_counts {
            let judgment = Judgment::parse_count(&raw_judgment, note_type)?;
            let next = result
                .get(note_type, judgment)
                .unwrap_or(0)
                .checked_add(count)
                .ok_or_else(|| AdapterError::input("judgment constraint count overflow"))?;
            result.set(note_type, judgment, Some(next));
        }
    }
    Ok(result)
}

fn note_totals(raw: std::collections::HashMap<String, u32>) -> Result<NoteTotals, AdapterError> {
    let mut result = NoteTotals::default();
    for (raw_note_type, count) in raw {
        let note_type = NoteType::from_str(&raw_note_type)?;
        let next = result
            .get(note_type)
            .checked_add(count)
            .ok_or_else(|| AdapterError::input("note total overflow"))?;
        result.set(note_type, next);
    }
    Ok(result)
}

fn apply_restrictions(
    raw: &std::collections::HashMap<String, OneOrMany>,
    destination: &mut [JudgmentSet; NOTE_TYPE_COUNT],
) -> Result<(), AdapterError> {
    let mut specified = [None; NOTE_TYPE_COUNT];
    for (raw_note_type, raw_values) in raw {
        let note_type = NoteType::from_str(raw_note_type)?;
        let set = specified[note_type.index()].get_or_insert(JudgmentSet::EMPTY);
        for token in raw_values.values() {
            if let Ok(group) = JudgmentGroup::from_str(token) {
                for judgment in group.judgments().iter() {
                    set.insert(judgment);
                }
            } else {
                set.insert(Judgment::parse_count(token, note_type)?);
            }
        }
    }
    for note_type in NoteType::ALL {
        if let Some(set) = specified[note_type.index()] {
            destination[note_type.index()] = set;
        }
    }
    Ok(())
}

fn raw_target(arguments: &FindCombinationsArgs) -> Result<SearchTarget, AdapterError> {
    if let Some(target) = &arguments.target_score {
        if arguments.min_score.is_some() || arguments.max_score.is_some() {
            return Err(AdapterError::input(
                "use either target_score or min_score/max_score, not both",
            ));
        }
        return Ok(SearchTarget::RawExact(raw_integer(target, "target_score")?));
    }
    if arguments.min_score.is_none() && arguments.max_score.is_none() {
        return Err(AdapterError::input(
            "target_score or min_score/max_score is required",
        ));
    }
    Ok(SearchTarget::RawRange {
        min: arguments
            .min_score
            .as_ref()
            .map(|value| raw_integer(value, "min_score"))
            .transpose()?,
        max: arguments
            .max_score
            .as_ref()
            .map(|value| raw_integer(value, "max_score"))
            .transpose()?,
    })
}

fn achievement_target(arguments: &FindCombinationsArgs) -> Result<SearchTarget, AdapterError> {
    let named_target = first_value([
        &arguments.target_acc,
        &arguments.target_percent,
        &arguments.target_percentage,
        &arguments.target_dxacc,
        &arguments.target_oldacc,
    ]);
    let target = named_target
        .map(percentage_decimal)
        .or_else(|| arguments.target_score.as_ref().map(percentage_score));
    let min = arguments
        .min_score
        .as_ref()
        .map(percentage_score)
        .or_else(|| {
            first_value([
                &arguments.min_acc,
                &arguments.min_percent,
                &arguments.min_percentage,
            ])
            .map(percentage_decimal)
        });
    let max = arguments
        .max_score
        .as_ref()
        .map(percentage_score)
        .or_else(|| {
            first_value([
                &arguments.max_acc,
                &arguments.max_percent,
                &arguments.max_percentage,
            ])
            .map(percentage_decimal)
        });
    if let Some(target) = target {
        if min.is_some() || max.is_some() {
            return Err(AdapterError::input(
                "use either target_score/target_acc or min_score/max_score, not both",
            ));
        }
        Ok(SearchTarget::PercentageExact(target))
    } else {
        Ok(SearchTarget::PercentageRange { min, max })
    }
}

fn raw_integer(value: &ScoreValue, field: &str) -> Result<i128, AdapterError> {
    let parsed = match value {
        ScoreValue::Integer(value) => Some(*value),
        ScoreValue::Text(value) if value.trim().bytes().all(|byte| byte.is_ascii_digit()) => {
            value.trim().parse::<i128>().ok()
        }
        ScoreValue::Number(value) => value.as_i128(),
        ScoreValue::Text(_) => None,
    };
    parsed
        .filter(|value| *value >= 0)
        .ok_or_else(|| AdapterError::input(format!("{field} must be a non-negative integer")))
}

fn percentage_score(value: &ScoreValue) -> PercentageInput {
    match value {
        ScoreValue::Integer(value) => PercentageInput::Scaled(*value),
        ScoreValue::Number(value) => match value.as_i128() {
            Some(value) => PercentageInput::Scaled(value),
            None => PercentageInput::Decimal(value.to_string()),
        },
        ScoreValue::Text(value)
            if !value.contains('.') && value.trim().bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            match value.trim().parse::<i128>() {
                Ok(value) => PercentageInput::Scaled(value),
                Err(_) => PercentageInput::Decimal(value.trim().to_owned()),
            }
        }
        ScoreValue::Text(_) => PercentageInput::Decimal(value.text()),
    }
}

fn percentage_decimal(value: &ScoreValue) -> PercentageInput {
    PercentageInput::Decimal(value.text())
}

fn first_value<const N: usize>(values: [&Option<ScoreValue>; N]) -> Option<&ScoreValue> {
    values.into_iter().find_map(Option::as_ref)
}

fn agreeing_bool<const N: usize>(
    message: &str,
    values: [Option<bool>; N],
) -> Result<Option<bool>, AdapterError> {
    agreeing_value(message, values)
}

fn agreeing_u32<const N: usize>(
    message: &str,
    values: [Option<u32>; N],
) -> Result<Option<u32>, AdapterError> {
    agreeing_value(message, values)
}

fn agreeing_value<T: Copy + Eq, const N: usize>(
    message: &str,
    values: [Option<T>; N],
) -> Result<Option<T>, AdapterError> {
    let mut provided = values.into_iter().flatten();
    let first = provided.next();
    if first.is_some_and(|first| provided.any(|value| value != first)) {
        Err(AdapterError::input(message))
    } else {
        Ok(first)
    }
}

fn display_mode(value: &str) -> Result<DisplayMode, AdapterError> {
    match value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "floor" => Ok(DisplayMode::Floor),
        "half_up" => Ok(DisplayMode::HalfUp),
        "exact" => Ok(DisplayMode::Exact),
        _ => Err(AdapterError::input(
            "display_mode must be one of: floor, half_up, exact",
        )),
    }
}
