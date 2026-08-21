use super::rational::Rational;
use super::{
    Judgment, NoteTotals, NoteType, PerNoteScores, PercentageDetails, ScoreCountRequest,
    ScoreCountResult, ScoreMode, ScoreRow, ScoreTotals, ScoringError, SelectedTotal,
};

pub fn score_counts(request: &ScoreCountRequest) -> Result<ScoreCountResult, ScoringError> {
    validate_display_digits(request.display_digits)?;

    let mut rows: Vec<ScoreRow> = Vec::new();
    let mut base = 0_i128;
    let mut break_bonus = 0_i128;
    let mut old_score = 0_i128;
    let mut dx_score = 0_i128;
    let mut selected_raw = 0_i128;

    for note_type in NoteType::ALL {
        for judgment in Judgment::ALL {
            let count = request.counts.get(note_type, judgment);
            let count_i128 = i128::from(count);
            let per_note = per_note_scores(note_type, judgment);
            let contribution = PerNoteScores {
                base: checked_mul(per_note.base, count_i128)?,
                break_bonus: checked_mul(per_note.break_bonus, count_i128)?,
                old_score: checked_mul(per_note.old_score, count_i128)?,
                dx_score: checked_mul(per_note.dx_score, count_i128)?,
            };
            base = checked_add(base, contribution.base)?;
            break_bonus = checked_add(break_bonus, contribution.break_bonus)?;
            old_score = checked_add(old_score, contribution.old_score)?;
            dx_score = checked_add(dx_score, contribution.dx_score)?;

            let selected_contribution = request
                .score_mode
                .filter(|mode| !mode.is_achievement())
                .map(|mode| checked_mul(score_value(note_type, judgment, mode), count_i128))
                .transpose()?;
            if let Some(selected) = selected_contribution {
                selected_raw = checked_add(selected_raw, selected)?;
            }

            if request.include_zero || count != 0 {
                let display_name = judgment.display_name(note_type);
                if let Some(row) = rows
                    .iter_mut()
                    .find(|row| row.note_type == note_type && row.judgment == display_name)
                {
                    row.count = row
                        .count
                        .checked_add(count)
                        .ok_or(ScoringError::Arithmetic("score row count overflow"))?;
                    row.contribution.base = checked_add(row.contribution.base, contribution.base)?;
                    row.contribution.break_bonus =
                        checked_add(row.contribution.break_bonus, contribution.break_bonus)?;
                    row.contribution.old_score =
                        checked_add(row.contribution.old_score, contribution.old_score)?;
                    row.contribution.dx_score =
                        checked_add(row.contribution.dx_score, contribution.dx_score)?;
                    if let Some(selected) = selected_contribution {
                        row.selected_contribution = Some(checked_add(
                            row.selected_contribution.unwrap_or(0),
                            selected,
                        )?);
                    }
                } else {
                    rows.push(ScoreRow {
                        note_type,
                        judgment: display_name.to_owned(),
                        count,
                        per_note,
                        contribution,
                        selected_contribution,
                    });
                }
            }
        }
    }

    let note_totals = note_totals_from_counts(&request.counts)?;
    let old_achievement = old_achievement(old_score, &note_totals, request.display_digits)?;
    let dx_achievement = dx_achievement(base, break_bonus, &note_totals, request.display_digits)?;
    let selected_mode = match request.score_mode {
        None => None,
        Some(mode) if !mode.is_achievement() => Some(SelectedTotal::Raw(selected_raw)),
        Some(ScoreMode::OldAchievement) => old_achievement.clone().map(SelectedTotal::Percentage),
        Some(ScoreMode::DxAchievement) => dx_achievement.clone().map(SelectedTotal::Percentage),
        Some(_) => None,
    };

    Ok(ScoreCountResult {
        display_digits: request.display_digits,
        note_totals,
        rows,
        totals: ScoreTotals {
            base,
            break_bonus,
            old_score,
            dx_score,
            old_achievement,
            dx_achievement,
            selected_mode,
        },
        score_mode: request.score_mode,
    })
}

pub(crate) fn validate_display_digits(display_digits: u32) -> Result<(), ScoringError> {
    if (1..=8).contains(&display_digits) {
        Ok(())
    } else {
        Err(ScoringError::InvalidDisplayDigits(display_digits))
    }
}

pub(crate) fn max_base_score(note_totals: &NoteTotals) -> Result<i128, ScoringError> {
    let tap_touch = i128::from(note_totals.get(NoteType::Tap))
        .checked_add(i128::from(note_totals.get(NoteType::Touch)))
        .ok_or(ScoringError::Arithmetic("maximum base score overflow"))?;
    let values = [
        checked_mul(tap_touch, 500)?,
        checked_mul(i128::from(note_totals.get(NoteType::Hold)), 1_000)?,
        checked_mul(i128::from(note_totals.get(NoteType::Slide)), 1_500)?,
        checked_mul(i128::from(note_totals.get(NoteType::Break)), 2_500)?,
    ];
    checked_sum(values)
}

pub(crate) fn max_old_score(note_totals: &NoteTotals) -> Result<i128, ScoringError> {
    checked_add(
        max_base_score(note_totals)?,
        checked_mul(i128::from(note_totals.get(NoteType::Break)), 100)?,
    )
}

pub(crate) const fn score_value(
    note_type: NoteType,
    judgment: Judgment,
    score_mode: ScoreMode,
) -> i128 {
    match score_mode {
        ScoreMode::Base => base_value(note_type, judgment),
        ScoreMode::BreakBonus => break_bonus_value(note_type, judgment),
        ScoreMode::OldScore => old_score_value(note_type, judgment),
        ScoreMode::DxScore => dx_score_value(judgment),
        ScoreMode::OldAchievement | ScoreMode::DxAchievement => 0,
    }
}

pub(crate) fn percentage_details(
    value: Rational,
    display_digits: u32,
) -> Result<PercentageDetails, ScoringError> {
    let (display_floor, scaled_floor) = value.display_floor(display_digits)?;
    let (display_half_up, scaled_half_up) = value.display_half_up(display_digits)?;
    Ok(PercentageDetails {
        raw: value.decimal_string(12)?,
        display_floor,
        display_half_up,
        scaled_floor,
        scaled_half_up,
    })
}

fn note_totals_from_counts(counts: &super::JudgmentCounts) -> Result<NoteTotals, ScoringError> {
    let mut totals = NoteTotals::default();
    for note_type in NoteType::ALL {
        totals.set(note_type, counts.note_total(note_type)?);
    }
    Ok(totals)
}

fn old_achievement(
    old_score: i128,
    note_totals: &NoteTotals,
    display_digits: u32,
) -> Result<Option<PercentageDetails>, ScoringError> {
    let denominator = max_base_score(note_totals)?;
    if denominator <= 0 {
        return Ok(None);
    }
    let numerator = checked_mul(old_score, 100)?;
    Ok(Some(percentage_details(
        Rational::new(numerator, denominator)?,
        display_digits,
    )?))
}

fn dx_achievement(
    base_score: i128,
    break_bonus_score: i128,
    note_totals: &NoteTotals,
    display_digits: u32,
) -> Result<Option<PercentageDetails>, ScoringError> {
    let max_base = max_base_score(note_totals)?;
    if max_base <= 0 {
        return Ok(None);
    }
    let mut value = Rational::new(checked_mul(base_score, 100)?, max_base)?;
    let max_break_bonus = checked_mul(i128::from(note_totals.get(NoteType::Break)), 100)?;
    if max_break_bonus != 0 {
        value = value.add(Rational::new(break_bonus_score, max_break_bonus)?)?;
    }
    Ok(Some(percentage_details(value, display_digits)?))
}

fn per_note_scores(note_type: NoteType, judgment: Judgment) -> PerNoteScores {
    PerNoteScores {
        base: base_value(note_type, judgment),
        break_bonus: break_bonus_value(note_type, judgment),
        old_score: old_score_value(note_type, judgment),
        dx_score: dx_score_value(judgment),
    }
}

const fn base_value(note_type: NoteType, judgment: Judgment) -> i128 {
    match note_type {
        NoteType::Tap | NoteType::Touch => match judgment {
            Judgment::Miss => 0,
            Judgment::Good => 250,
            Judgment::GreatLow | Judgment::GreatMid | Judgment::GreatHigh => 400,
            Judgment::PerfectLow | Judgment::PerfectHigh | Judgment::Critical => 500,
        },
        NoteType::Hold => match judgment {
            Judgment::Miss => 0,
            Judgment::Good => 500,
            Judgment::GreatLow | Judgment::GreatMid | Judgment::GreatHigh => 800,
            Judgment::PerfectLow | Judgment::PerfectHigh | Judgment::Critical => 1_000,
        },
        NoteType::Slide => match judgment {
            Judgment::Miss => 0,
            Judgment::Good => 750,
            Judgment::GreatLow | Judgment::GreatMid | Judgment::GreatHigh => 1_200,
            Judgment::PerfectLow | Judgment::PerfectHigh | Judgment::Critical => 1_500,
        },
        NoteType::Break => match judgment {
            Judgment::Miss => 0,
            Judgment::Good => 1_000,
            Judgment::GreatLow => 1_250,
            Judgment::GreatMid => 1_500,
            Judgment::GreatHigh => 2_000,
            Judgment::PerfectLow | Judgment::PerfectHigh | Judgment::Critical => 2_500,
        },
    }
}

const fn break_bonus_value(note_type: NoteType, judgment: Judgment) -> i128 {
    if !matches!(note_type, NoteType::Break) {
        return 0;
    }
    match judgment {
        Judgment::Miss => 0,
        Judgment::Good => 30,
        Judgment::GreatLow | Judgment::GreatMid | Judgment::GreatHigh => 40,
        Judgment::PerfectLow => 50,
        Judgment::PerfectHigh => 75,
        Judgment::Critical => 100,
    }
}

const fn old_score_value(note_type: NoteType, judgment: Judgment) -> i128 {
    if !matches!(note_type, NoteType::Break) {
        return base_value(note_type, judgment);
    }
    match judgment {
        Judgment::Miss => 0,
        Judgment::Good => 1_000,
        Judgment::GreatLow => 1_250,
        Judgment::GreatMid => 1_500,
        Judgment::GreatHigh => 2_000,
        Judgment::PerfectLow => 2_500,
        Judgment::PerfectHigh => 2_550,
        Judgment::Critical => 2_600,
    }
}

const fn dx_score_value(judgment: Judgment) -> i128 {
    match judgment {
        Judgment::Miss | Judgment::Good => 0,
        Judgment::GreatLow | Judgment::GreatMid | Judgment::GreatHigh => 1,
        Judgment::PerfectLow | Judgment::PerfectHigh => 2,
        Judgment::Critical => 3,
    }
}

fn checked_sum(values: impl IntoIterator<Item = i128>) -> Result<i128, ScoringError> {
    values.into_iter().try_fold(0_i128, checked_add)
}

fn checked_add(left: i128, right: i128) -> Result<i128, ScoringError> {
    left.checked_add(right)
        .ok_or(ScoringError::Arithmetic("score addition overflow"))
}

fn checked_mul(left: i128, right: i128) -> Result<i128, ScoringError> {
    left.checked_mul(right)
        .ok_or(ScoringError::Arithmetic("score multiplication overflow"))
}

#[cfg(test)]
mod tests {
    use super::score_counts;
    use crate::scoring::{
        Judgment, JudgmentCounts, NoteType, ScoreCountRequest, ScoreMode, ScoringError,
        SelectedTotal,
    };

    #[test]
    fn score_counts_matches_python_break_and_regular_values() -> Result<(), ScoringError> {
        let mut counts = JudgmentCounts::empty();
        counts.set(NoteType::Tap, Judgment::GreatMid, 2);
        counts.set(NoteType::Break, Judgment::PerfectHigh, 1);
        counts.set(NoteType::Break, Judgment::Critical, 1);
        let result = score_counts(&ScoreCountRequest {
            counts,
            score_mode: Some(ScoreMode::OldScore),
            display_digits: 4,
            include_zero: false,
        })?;

        assert_eq!(result.totals.base, 5_800);
        assert_eq!(result.totals.break_bonus, 175);
        assert_eq!(result.totals.old_score, 5_950);
        assert_eq!(result.totals.dx_score, 7);
        assert_eq!(result.totals.selected_mode, Some(SelectedTotal::Raw(5_950)));
        assert_eq!(result.note_totals.get(NoteType::Tap), 2);
        assert_eq!(result.note_totals.get(NoteType::Break), 2);
        Ok(())
    }

    #[test]
    fn non_break_rows_collapse_great_variants() -> Result<(), ScoringError> {
        let mut counts = JudgmentCounts::empty();
        counts.set(NoteType::Tap, Judgment::GreatLow, 1);
        counts.set(NoteType::Tap, Judgment::GreatHigh, 2);
        let result = score_counts(&ScoreCountRequest {
            counts,
            ..ScoreCountRequest::default()
        })?;
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].judgment, "great");
        assert_eq!(result.rows[0].count, 3);
        Ok(())
    }
}
