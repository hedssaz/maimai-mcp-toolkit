use std::{error::Error, fmt};

use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::achievement::AchievementRate;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ChartConstant(Decimal);

impl ChartConstant {
    pub fn from_hundredths(value: u32) -> Result<Self, RatingError> {
        Self::from_decimal(Decimal::new(i64::from(value), 2))
    }

    pub fn from_decimal_str(value: &str) -> Result<Self, RatingError> {
        Self::from_decimal(parse_decimal(value, 16, "chartConstant")?)
    }

    pub const fn value(self) -> Decimal {
        self.0
    }

    fn from_decimal(value: Decimal) -> Result<Self, RatingError> {
        if value > Decimal::from(100_u32) {
            return Err(RatingError::OutOfRange {
                field: "chartConstant",
            });
        }
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for ChartConstant {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_decimal_str(&value).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AchievementRank {
    SssPlus,
    Sss,
    SsPlus,
    Ss,
    SPlus,
    S,
    Aaa,
    Aa,
    A,
    Bbb,
    Bb,
    B,
    C,
    D,
}

impl AchievementRank {
    pub const fn legacy_code(self) -> &'static str {
        match self {
            Self::SssPlus => "sssp",
            Self::Sss => "sss",
            Self::SsPlus => "ssp",
            Self::Ss => "ss",
            Self::SPlus => "sp",
            Self::S => "s",
            Self::Aaa => "aaa",
            Self::Aa => "aa",
            Self::A => "a",
            Self::Bbb => "bbb",
            Self::Bb => "bb",
            Self::B => "b",
            Self::C => "c",
            Self::D => "d",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RatingBreakdown {
    /// 旧曲 B35 总和；对应旧输出的 `ratingBreakdown.sd`。
    pub b35: u32,
    /// 新曲 B15 总和；对应旧输出的 `ratingBreakdown.dx`。
    pub b15: u32,
    pub total: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RatingError {
    InvalidDecimal {
        field: &'static str,
        value: String,
    },
    TooPrecise {
        field: &'static str,
        decimal_places: u8,
    },
    OutOfRange {
        field: &'static str,
    },
    ArithmeticOverflow,
}

impl fmt::Display for RatingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDecimal { field, value } => {
                write!(formatter, "{field} 不是有效的非负十进制数：{value}")
            }
            Self::TooPrecise {
                field,
                decimal_places,
            } => write!(formatter, "{field} 最多支持 {decimal_places} 位小数"),
            Self::OutOfRange { field } => write!(formatter, "{field} 超出支持范围"),
            Self::ArithmeticOverflow => formatter.write_str("rating 汇总溢出"),
        }
    }
}

impl Error for RatingError {}

/// 返回旧 `achievement_rate` 使用的等级代码所对应的强类型分段。
pub const fn achievement_rank(achievements: AchievementRate) -> AchievementRank {
    match achievements.ten_thousandths() {
        value if value >= 1_005_000 => AchievementRank::SssPlus,
        value if value >= 1_000_000 => AchievementRank::Sss,
        value if value >= 995_000 => AchievementRank::SsPlus,
        value if value >= 990_000 => AchievementRank::Ss,
        value if value >= 980_000 => AchievementRank::SPlus,
        value if value >= 970_000 => AchievementRank::S,
        value if value >= 940_000 => AchievementRank::Aaa,
        value if value >= 900_000 => AchievementRank::Aa,
        value if value >= 800_000 => AchievementRank::A,
        value if value >= 750_000 => AchievementRank::Bbb,
        value if value >= 700_000 => AchievementRank::Bb,
        value if value >= 600_000 => AchievementRank::B,
        value if value >= 500_000 => AchievementRank::C,
        _ => AchievementRank::D,
    }
}

/// 评级系数乘以 10 后的整数值，例如 `22.4` 返回 `224`。
pub const fn coefficient_tenths(achievements: AchievementRate) -> u16 {
    match achievements.ten_thousandths() {
        value if value >= 1_005_000 => 224,
        value if value >= 1_000_000 => 216,
        value if value >= 995_000 => 211,
        value if value >= 990_000 => 208,
        value if value >= 980_000 => 203,
        value if value >= 970_000 => 200,
        value if value >= 940_000 => 168,
        value if value >= 900_000 => 152,
        value if value >= 800_000 => 136,
        value if value >= 750_000 => 120,
        value if value >= 700_000 => 112,
        value if value >= 600_000 => 96,
        value if value >= 500_000 => 80,
        value if value >= 400_000 => 64,
        value if value >= 300_000 => 48,
        value if value >= 200_000 => 32,
        value if value >= 100_000 => 16,
        _ => 0,
    }
}

/// 单曲 DX Rating：
/// `floor(ds * min(achievements, 100.5) / 100 * coefficient)`。
///
/// 定数和达成率均使用十进制定点数，最后明确向下取整。
pub fn single_song_rating(
    ds: ChartConstant,
    achievements: AchievementRate,
) -> Result<u32, RatingError> {
    let capped = achievements.capped();
    let achievement = Decimal::new(i64::from(capped.ten_thousandths()), 4);
    let coefficient = Decimal::new(i64::from(coefficient_tenths(capped)), 1);
    (ds.value() * achievement / Decimal::from(100_u32) * coefficient)
        .floor()
        .to_u32()
        .ok_or(RatingError::ArithmeticOverflow)
}

/// 旧函数名对应的明确别名，便于迁移调用方。
pub fn maimai_dx_ra(ds: ChartConstant, achievements: AchievementRate) -> Result<u32, RatingError> {
    single_song_rating(ds, achievements)
}

/// 降序取前 `limit` 个单曲 rating 并求和。
pub fn top_rating_sum(ratings: &[u32], limit: usize) -> Result<u32, RatingError> {
    let mut ordered = ratings.to_vec();
    ordered.sort_unstable_by(|left, right| right.cmp(left));
    ordered
        .into_iter()
        .take(limit)
        .try_fold(0_u32, |total, rating| {
            total
                .checked_add(rating)
                .ok_or(RatingError::ArithmeticOverflow)
        })
}

/// 分别截取 B35/B15 后汇总，输入可以包含超过 35/15 条的候选成绩。
pub fn b50_rating_breakdown(
    b35_candidates: &[u32],
    b15_candidates: &[u32],
) -> Result<RatingBreakdown, RatingError> {
    let b35 = top_rating_sum(b35_candidates, 35)?;
    let b15 = top_rating_sum(b15_candidates, 15)?;
    let total = b35
        .checked_add(b15)
        .ok_or(RatingError::ArithmeticOverflow)?;
    Ok(RatingBreakdown { b35, b15, total })
}

fn parse_decimal(
    input: &str,
    decimal_places: u8,
    field: &'static str,
) -> Result<Decimal, RatingError> {
    let value = input.trim();
    let value = value.strip_prefix('+').unwrap_or(value);
    let parsed = value
        .parse::<Decimal>()
        .map_err(|_| RatingError::InvalidDecimal {
            field,
            value: input.to_owned(),
        })?
        .normalize();
    if parsed.is_sign_negative() {
        return Err(RatingError::InvalidDecimal {
            field,
            value: input.to_owned(),
        });
    }
    if parsed.scale() > u32::from(decimal_places) {
        return Err(RatingError::TooPrecise {
            field,
            decimal_places,
        });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::{
        AchievementRank, AchievementRate, ChartConstant, RatingBreakdown, RatingError,
        achievement_rank, b50_rating_breakdown, coefficient_tenths, maimai_dx_ra,
    };

    fn ds(value: &str) -> Result<ChartConstant, RatingError> {
        ChartConstant::from_decimal_str(value)
    }

    fn achievement(value: &str) -> Result<AchievementRate, RatingError> {
        AchievementRate::from_decimal_str(value)
    }

    #[test]
    fn matches_known_single_song_examples_without_float_rounding() -> Result<(), RatingError> {
        assert_eq!(maimai_dx_ra(ds("14.5")?, achievement("100.5351")?)?, 326);
        assert_eq!(maimai_dx_ra(ds("13.0")?, achievement("100.0")?)?, 280);
        assert_eq!(maimai_dx_ra(ds("14.5")?, achievement("101.0")?)?, 326);
        assert_eq!(maimai_dx_ra(ds("13.0")?, achievement("99.5")?)?, 272);
        assert_eq!(maimai_dx_ra(ds("13.7")?, achievement("100.6")?)?, 308);
        assert_eq!(maimai_dx_ra(ds("13.6")?, achievement("100.2")?)?, 294);
        assert_eq!(
            maimai_dx_ra(ds("13.700590667988907")?, achievement("100.5")?)?,
            308
        );
        assert_eq!(
            maimai_dx_ra(ds("14.449996086031119")?, achievement("100.2")?)?,
            312
        );
        Ok(())
    }

    #[test]
    fn keeps_all_coefficient_boundaries() -> Result<(), RatingError> {
        let boundaries = [
            ("100.5", 224),
            ("100.0", 216),
            ("99.5", 211),
            ("99.0", 208),
            ("98.0", 203),
            ("97.0", 200),
            ("94.0", 168),
            ("90.0", 152),
            ("80.0", 136),
            ("75.0", 120),
            ("70.0", 112),
            ("60.0", 96),
            ("50.0", 80),
            ("40.0", 64),
            ("30.0", 48),
            ("20.0", 32),
            ("10.0", 16),
            ("9.9999", 0),
        ];
        for (value, expected) in boundaries {
            assert_eq!(coefficient_tenths(achievement(value)?), expected, "{value}");
        }
        Ok(())
    }

    #[test]
    fn exposes_legacy_achievement_rank_codes() -> Result<(), RatingError> {
        assert_eq!(
            achievement_rank(achievement("100.5")?),
            AchievementRank::SssPlus
        );
        assert_eq!(achievement_rank(achievement("99.5")?).legacy_code(), "ssp");
        assert_eq!(achievement_rank(achievement("50.0")?).legacy_code(), "c");
        assert_eq!(achievement_rank(achievement("49.9999")?).legacy_code(), "d");
        Ok(())
    }

    #[test]
    fn b35_and_b15_use_only_the_highest_candidates() -> Result<(), RatingError> {
        let b35: Vec<u32> = (1..=40).collect();
        let b15: Vec<u32> = (101..=120).collect();
        let result = b50_rating_breakdown(&b35, &b15)?;

        assert_eq!(
            result,
            RatingBreakdown {
                b35: (6..=40).sum(),
                b15: (106..=120).sum(),
                total: (6..=40).sum::<u32>() + (106..=120).sum::<u32>(),
            }
        );
        Ok(())
    }

    #[test]
    fn rejects_precision_that_would_require_rounding() {
        assert_eq!(
            ChartConstant::from_decimal_str("14.12345678901234567"),
            Err(RatingError::TooPrecise {
                field: "chartConstant",
                decimal_places: 16,
            })
        );
        assert_eq!(
            ChartConstant::from_decimal_str("14.5000"),
            ChartConstant::from_decimal_str("14.5")
        );
        assert_eq!(
            AchievementRate::from_decimal_str("101.0001"),
            Err(RatingError::OutOfRange {
                field: "achievements",
            })
        );
    }

    #[test]
    fn serde_preserves_shapes_and_enforces_ranges() -> Result<(), Box<dyn Error>> {
        let constant = ChartConstant::from_decimal_str("13.7000000000000001")?;
        let achievement = AchievementRate::from_decimal_str("100.5351")?;

        let constant_json = serde_json::to_string(&constant)?;
        let achievement_json = serde_json::to_string(&achievement)?;
        assert_eq!(constant_json, r#""13.7000000000000001""#);
        assert_eq!(achievement_json, "1005351");
        assert_eq!(
            serde_json::from_str::<ChartConstant>(&constant_json)?,
            constant
        );
        assert_eq!(
            serde_json::from_str::<AchievementRate>(&achievement_json)?,
            achievement
        );
        assert!(serde_json::from_str::<ChartConstant>(r#""101""#).is_err());
        assert!(serde_json::from_str::<ChartConstant>(r#""-0.1""#).is_err());
        assert!(serde_json::from_str::<AchievementRate>("1010001").is_err());
        Ok(())
    }
}
