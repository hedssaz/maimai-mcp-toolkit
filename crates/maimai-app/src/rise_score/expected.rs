use maimai_core::{AchievementRate, ChartConstant, coefficient_tenths};
use rust_decimal::Decimal;

use crate::scores::B50Chart;

pub(super) fn target_abilities(
    records: &[B50Chart],
    targets: &[AchievementRate; 4],
    floor: u32,
) -> [(AchievementRate, Decimal); 4] {
    targets.map(|target| (target, target_ability(records, target, floor)))
}

fn target_ability(records: &[B50Chart], target: AchievementRate, floor: u32) -> Decimal {
    let mut achieved = records
        .iter()
        .filter(|record| {
            record
                .achievements
                .and_then(|value| value.ranked())
                .is_some_and(|value| value >= target)
        })
        .filter_map(|record| record.constant.map(ChartConstant::value))
        .collect::<Vec<_>>();
    achieved.sort();
    let observed = match achieved.len() {
        8.. => percentile(&achieved, 82, 100),
        3.. => percentile(&achieved, 70, 100),
        1.. => achieved.last().copied().unwrap_or(Decimal::ZERO) - Decimal::new(2, 1),
        0 => Decimal::ZERO,
    };
    observed.max(floor_constant(floor, target))
}

fn percentile(values: &[Decimal], numerator: u32, denominator: u32) -> Decimal {
    if values.len() <= 1 || denominator == 0 {
        return values.first().copied().unwrap_or(Decimal::ZERO);
    }
    let span = values.len() - 1;
    let scaled = u64::try_from(span).unwrap_or(u64::MAX) * u64::from(numerator);
    let lower = usize::try_from(scaled / u64::from(denominator)).unwrap_or(0);
    let remainder = scaled % u64::from(denominator);
    let upper = (lower + 1).min(values.len() - 1);
    let fraction = Decimal::from(remainder) / Decimal::from(denominator);
    values[lower] * (Decimal::ONE - fraction) + values[upper] * fraction
}

fn floor_constant(rating: u32, target: AchievementRate) -> Decimal {
    let achievement = Decimal::new(i64::from(target.ten_thousandths()), 4);
    let coefficient = Decimal::new(i64::from(coefficient_tenths(target)), 1);
    let denominator = achievement * coefficient;
    if denominator.is_zero() {
        Decimal::ZERO
    } else {
        Decimal::from(rating) * Decimal::from(100_u32) / denominator
    }
}

pub(super) fn target_allowed(target: AchievementRate, old: Option<AchievementRate>) -> bool {
    let target = target.ten_thousandths();
    match old.map(AchievementRate::ten_thousandths).unwrap_or(0) {
        0 => target <= 1_000_000,
        value if value < 970_000 => target <= 995_000,
        value if value < 995_000 => target <= 1_000_000,
        _ => true,
    }
}

pub(super) fn margin_limit(target: AchievementRate, old: Option<AchievementRate>) -> Decimal {
    let old_value = old.map(AchievementRate::ten_thousandths).unwrap_or(0);
    let target_value = target.ten_thousandths();
    let close = old_value > 0 && old_value.saturating_add(8_000) >= target_value;
    let bonus = if close {
        Decimal::new(15, 2)
    } else {
        Decimal::ZERO
    };
    let base = if old_value == 0 {
        match target_value {
            1_005_000.. => Decimal::ZERO,
            1_000_000.. => Decimal::new(15, 2),
            995_000.. => Decimal::ZERO,
            _ => Decimal::new(25, 2),
        }
    } else {
        match target_value {
            1_005_000.. => Decimal::new(25, 2),
            1_000_000.. => Decimal::new(35, 2),
            995_000.. => Decimal::new(55, 2),
            _ => Decimal::new(75, 2),
        }
    };
    base + bonus
}

pub(super) fn probability(
    constant: ChartConstant,
    target: AchievementRate,
    ability: Decimal,
    old: Option<AchievementRate>,
) -> Decimal {
    let margin = ability - constant.value();
    let mut result = Decimal::new(62, 2) + margin * Decimal::new(95, 2);
    if target.ten_thousandths() >= 1_005_000 {
        result -= Decimal::new(5, 2);
    } else if target.ten_thousandths() <= 990_000 {
        result += Decimal::new(6, 2);
    }
    if let Some(old) = old {
        let old = old.ten_thousandths();
        let target = target.ten_thousandths();
        result += if old.saturating_add(1_000) >= target {
            Decimal::new(18, 2)
        } else if old.saturating_add(6_000) >= target {
            Decimal::new(10, 2)
        } else if old.saturating_add(15_000) >= target {
            Decimal::new(4, 2)
        } else {
            Decimal::ZERO
        };
    }
    result.clamp(Decimal::new(5, 2), Decimal::new(98, 2))
}

pub(super) struct ExpectedScoreInput {
    pub actual_gain: u32,
    pub over_floor: u32,
    pub replacement_floor: u32,
    pub recommendation_floor: u32,
    pub constant: ChartConstant,
    pub ability: Decimal,
    pub probability: Decimal,
    pub fit_bucket: u8,
}

pub(super) fn candidate_score(input: ExpectedScoreInput) -> Decimal {
    let under = (input.ability - input.constant.value() - Decimal::new(45, 2)).max(Decimal::ZERO);
    let over = (input.constant.value() - input.ability - Decimal::new(55, 2)).max(Decimal::ZERO);
    let fill_weight = if input.replacement_floor == 0 && input.recommendation_floor > 0 {
        Decimal::new(24, 2)
    } else {
        Decimal::new(85, 2)
    };
    Decimal::from(input.actual_gain) * input.probability * fill_weight
        + Decimal::from(input.over_floor) * Decimal::new(55, 1)
        + fit_bonus(input.fit_bucket)
        - under * Decimal::from(72_u32)
        - over * Decimal::from(110_u32)
}

fn fit_bonus(bucket: u8) -> Decimal {
    Decimal::from(match bucket {
        0 => 24,
        1 => 12,
        2 => 0,
        3 => -18,
        _ => -8,
    })
}
