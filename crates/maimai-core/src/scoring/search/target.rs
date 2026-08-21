use std::cmp::Ordering;

use num_integer::Integer;

use crate::scoring::calculate::{max_base_score, max_old_score, score_value};
use crate::scoring::rational::{Rational, parse_decimal, pow10};
use crate::scoring::{
    DisplayMode, Judgment, MetricDetails, NoteTotals, NoteType, PercentageInput, ScoreMode,
    ScoringError, SearchRequest, SearchTarget, TargetRange,
};

#[derive(Clone, Debug)]
pub(super) struct AchievementMetric {
    pub(super) denominator: i128,
    pub(super) max_metric: i128,
    pub(super) loss_values:
        [[i128; crate::scoring::types::JUDGMENT_COUNT]; crate::scoring::types::NOTE_TYPE_COUNT],
    pub(super) details: MetricDetails,
}

#[derive(Clone, Debug)]
pub(super) struct AchievementLossRange {
    pub(super) min_loss: i128,
    pub(super) max_loss: i128,
    pub(super) target_range: TargetRange,
}

pub(super) fn raw_target_range(target: &SearchTarget) -> Result<(i128, i128), ScoringError> {
    let (min_score, max_score) = match target {
        SearchTarget::RawExact(value) => (*value, *value),
        SearchTarget::RawRange { min, max } => {
            (min.unwrap_or(0), max.unwrap_or(1_000_000_000_000_000_000))
        }
        SearchTarget::PercentageExact(_) | SearchTarget::PercentageRange { .. } => {
            return Err(ScoringError::InvalidInput(
                "raw score modes require a raw score target".to_owned(),
            ));
        }
    };
    if min_score < 0 || max_score < 0 {
        return Err(ScoringError::InvalidInput(
            "raw score targets must be non-negative".to_owned(),
        ));
    }
    if min_score > max_score {
        return Err(ScoringError::InvalidInput(
            "min_score cannot be greater than max_score".to_owned(),
        ));
    }
    Ok((min_score, max_score))
}

pub(super) fn achievement_metric(
    mode: ScoreMode,
    totals: &NoteTotals,
) -> Result<AchievementMetric, ScoringError> {
    let max_base = max_base_score(totals)?;
    if max_base <= 0 {
        return Err(ScoringError::InvalidInput(format!(
            "{} requires at least one scoring note",
            mode.as_str()
        )));
    }
    match mode {
        ScoreMode::DxAchievement => dx_achievement_metric(totals, max_base),
        ScoreMode::OldAchievement => old_achievement_metric(totals, max_base),
        _ => Err(ScoringError::InvalidInput(
            "achievement metric requires oldacc or dxacc".to_owned(),
        )),
    }
}

fn dx_achievement_metric(
    totals: &NoteTotals,
    max_base: i128,
) -> Result<AchievementMetric, ScoringError> {
    let max_break_bonus = i128::from(totals.get(NoteType::Break))
        .checked_mul(100)
        .ok_or(ScoringError::Arithmetic("maximum break bonus overflow"))?;
    let denominator = if max_break_bonus == 0 {
        max_base
    } else {
        checked_lcm(max_base, max_break_bonus)?
    };
    let base_weight = denominator
        .checked_mul(100)
        .and_then(|value| value.checked_div(max_base))
        .ok_or(ScoringError::Arithmetic("base metric weight overflow"))?;
    let break_bonus_weight = if max_break_bonus == 0 {
        0
    } else {
        denominator / max_break_bonus
    };
    let max_metric = max_base
        .checked_mul(base_weight)
        .and_then(|value| {
            max_break_bonus
                .checked_mul(break_bonus_weight)
                .and_then(|bonus| value.checked_add(bonus))
        })
        .ok_or(ScoringError::Arithmetic(
            "maximum DX achievement metric overflow",
        ))?;
    let mut loss_values =
        [[0_i128; crate::scoring::types::JUDGMENT_COUNT]; crate::scoring::types::NOTE_TYPE_COUNT];
    for note_type in NoteType::ALL {
        let max_base_value = score_value(note_type, Judgment::Critical, ScoreMode::Base);
        let max_bonus_value: i128 = if note_type == NoteType::Break { 100 } else { 0 };
        let maximum = max_base_value
            .checked_mul(base_weight)
            .and_then(|value| {
                max_bonus_value
                    .checked_mul(break_bonus_weight)
                    .and_then(|bonus| value.checked_add(bonus))
            })
            .ok_or(ScoringError::Arithmetic("per-note DX metric overflow"))?;
        for judgment in Judgment::ALL {
            let earned_base = score_value(note_type, judgment, ScoreMode::Base)
                .checked_mul(base_weight)
                .ok_or(ScoringError::Arithmetic("per-note DX metric overflow"))?;
            let earned_bonus = score_value(note_type, judgment, ScoreMode::BreakBonus)
                .checked_mul(break_bonus_weight)
                .ok_or(ScoringError::Arithmetic("per-note DX metric overflow"))?;
            let earned = earned_base
                .checked_add(earned_bonus)
                .ok_or(ScoringError::Arithmetic("per-note DX metric overflow"))?;
            loss_values[note_type.index()][judgment.index()] = maximum
                .checked_sub(earned)
                .ok_or(ScoringError::Arithmetic("per-note DX loss overflow"))?;
        }
    }
    Ok(AchievementMetric {
        denominator,
        max_metric,
        loss_values,
        details: MetricDetails {
            max_base,
            max_break_bonus: Some(max_break_bonus),
            max_old: None,
            denominator,
            max_metric,
            base_weight: Some(base_weight),
            break_bonus_weight: Some(break_bonus_weight),
            min_loss: 0,
            max_loss: 0,
        },
    })
}

fn old_achievement_metric(
    totals: &NoteTotals,
    max_base: i128,
) -> Result<AchievementMetric, ScoringError> {
    let max_old = max_old_score(totals)?;
    let max_metric = max_old.checked_mul(100).ok_or(ScoringError::Arithmetic(
        "maximum old achievement metric overflow",
    ))?;
    let mut loss_values =
        [[0_i128; crate::scoring::types::JUDGMENT_COUNT]; crate::scoring::types::NOTE_TYPE_COUNT];
    for note_type in NoteType::ALL {
        let maximum = score_value(note_type, Judgment::Critical, ScoreMode::OldScore)
            .checked_mul(100)
            .ok_or(ScoringError::Arithmetic("per-note old metric overflow"))?;
        for judgment in Judgment::ALL {
            let earned = score_value(note_type, judgment, ScoreMode::OldScore)
                .checked_mul(100)
                .ok_or(ScoringError::Arithmetic("per-note old metric overflow"))?;
            loss_values[note_type.index()][judgment.index()] = maximum
                .checked_sub(earned)
                .ok_or(ScoringError::Arithmetic("per-note old loss overflow"))?;
        }
    }
    Ok(AchievementMetric {
        denominator: max_base,
        max_metric,
        loss_values,
        details: MetricDetails {
            max_base,
            max_break_bonus: None,
            max_old: Some(max_old),
            denominator: max_base,
            max_metric,
            base_weight: None,
            break_bonus_weight: None,
            min_loss: 0,
            max_loss: 0,
        },
    })
}

pub(super) fn achievement_loss_range(
    request: &SearchRequest,
    metric: &AchievementMetric,
) -> Result<AchievementLossRange, ScoringError> {
    let unit = Rational::new(1, pow10(request.display_digits)?)?;
    match &request.target {
        SearchTarget::PercentageExact(value) => {
            let target = percentage_input(value, request.display_digits, "target_score")?;
            let (lower, min_loss, max_loss, max, max_exclusive, mode_detail) =
                match request.display_mode {
                    DisplayMode::Floor => {
                        let upper = target.add(unit)?;
                        let min_loss = loss_against_bound(metric, upper)?
                            .floor()
                            .checked_add(1)
                            .ok_or(ScoringError::Arithmetic("loss lower bound overflow"))?;
                        let max_loss = loss_against_bound(metric, target)?.floor();
                        (
                            target,
                            min_loss,
                            max_loss,
                            None,
                            Some(upper.decimal_string(12)?),
                            format!(
                                "raw {} in [target, target + display unit)",
                                request.score_mode.as_str()
                            ),
                        )
                    }
                    DisplayMode::HalfUp => {
                        let half_unit = unit.divide_integer(2)?;
                        let lower = target.subtract(half_unit)?;
                        let upper = target.add(half_unit)?;
                        let min_loss = loss_against_bound(metric, upper)?
                            .floor()
                            .checked_add(1)
                            .ok_or(ScoringError::Arithmetic("loss lower bound overflow"))?;
                        let max_loss = loss_against_bound(metric, lower)?.floor();
                        (
                            lower,
                            min_loss,
                            max_loss,
                            None,
                            Some(upper.decimal_string(12)?),
                            format!(
                                "raw {} rounds half-up to target",
                                request.score_mode.as_str()
                            ),
                        )
                    }
                    DisplayMode::Exact => {
                        let target_metric = target.multiply_integer(metric.denominator)?;
                        if !target_metric.is_integer() {
                            let text = target.decimal_string(12)?;
                            return Ok(AchievementLossRange {
                            min_loss: 1,
                            max_loss: 0,
                            target_range: TargetRange::Percentage {
                                min: text.clone(),
                                max: Some(text),
                                max_exclusive: None,
                                mode_detail:
                                    "exact target is not representable for this chart denominator"
                                        .to_owned(),
                            },
                        });
                        }
                        let loss = metric
                            .max_metric
                            .checked_sub(target_metric.numerator())
                            .ok_or(ScoringError::Arithmetic("exact target loss overflow"))?;
                        (
                            target,
                            loss,
                            loss,
                            Some(target.decimal_string(12)?),
                            None,
                            format!("raw {} exactly equals target", request.score_mode.as_str()),
                        )
                    }
                };
            Ok(AchievementLossRange {
                min_loss: min_loss.max(0),
                max_loss,
                target_range: TargetRange::Percentage {
                    min: lower.decimal_string(12)?,
                    max,
                    max_exclusive,
                    mode_detail,
                },
            })
        }
        SearchTarget::PercentageRange { min, max } => {
            if min.is_none() && max.is_none() {
                return Err(ScoringError::InvalidInput(
                    "target_score, target_acc, or min_score/max_score is required".to_owned(),
                ));
            }
            let lower = match min {
                Some(value) => percentage_input(value, request.display_digits, "min_score")?,
                None => Rational::integer(0),
            };
            let upper = match max {
                Some(value) => percentage_input(value, request.display_digits, "max_score")?,
                None => Rational::integer(1_000_000_000),
            };
            if lower.cmp_checked(upper)? == Ordering::Greater {
                return Err(ScoringError::InvalidInput(
                    "minimum percentage cannot be greater than maximum percentage".to_owned(),
                ));
            }
            let min_loss = loss_against_bound(metric, upper)?.ceil().max(0);
            let max_loss = loss_against_bound(metric, lower)?.floor();
            Ok(AchievementLossRange {
                min_loss,
                max_loss,
                target_range: TargetRange::Percentage {
                    min: lower.decimal_string(12)?,
                    max: Some(upper.decimal_string(12)?),
                    max_exclusive: None,
                    mode_detail: format!(
                        "raw {} in inclusive min/max range",
                        request.score_mode.as_str()
                    ),
                },
            })
        }
        SearchTarget::RawExact(_) | SearchTarget::RawRange { .. } => {
            Err(ScoringError::InvalidInput(
                "achievement score modes require a percentage target".to_owned(),
            ))
        }
    }
}

fn loss_against_bound(
    metric: &AchievementMetric,
    bound: Rational,
) -> Result<Rational, ScoringError> {
    Rational::integer(metric.max_metric).subtract(bound.multiply_integer(metric.denominator)?)
}

fn percentage_input(
    input: &PercentageInput,
    display_digits: u32,
    field: &'static str,
) -> Result<Rational, ScoringError> {
    match input {
        PercentageInput::Scaled(value) => Rational::new(*value, pow10(display_digits)?),
        PercentageInput::Decimal(value) => parse_decimal(value, field),
    }
}

fn checked_lcm(left: i128, right: i128) -> Result<i128, ScoringError> {
    let divisor = left.gcd(&right);
    left.checked_div(divisor)
        .and_then(|value| value.checked_mul(right))
        .ok_or(ScoringError::Arithmetic("achievement denominator overflow"))
}
