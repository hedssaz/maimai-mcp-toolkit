use std::{error::Error, fmt};

use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::rating::RatingError;

const ACHIEVEMENT_SCALE: u32 = 10_000;
const ACHIEVEMENT_CAP: u32 = 100 * ACHIEVEMENT_SCALE + 5_000;
const ACHIEVEMENT_MAX: u32 = 101 * ACHIEVEMENT_SCALE;

/// 达成率以“百分数的小数点后四位”为最小单位。
///
/// 例如 `100.5351%` 保存为 `1_005_351`，计算全程不经过二进制浮点数。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AchievementRate(u32);

impl AchievementRate {
    pub fn from_ten_thousandths(value: u32) -> Result<Self, RatingError> {
        if value > ACHIEVEMENT_MAX {
            return Err(RatingError::OutOfRange {
                field: "achievements",
            });
        }
        Ok(Self(value))
    }

    pub fn from_decimal_str(value: &str) -> Result<Self, RatingError> {
        Self::from_ten_thousandths(decimal_units(value)?)
    }

    pub const fn ten_thousandths(self) -> u32 {
        self.0
    }

    pub const fn capped(self) -> Self {
        Self(if self.0 > ACHIEVEMENT_CAP {
            ACHIEVEMENT_CAP
        } else {
            self.0
        })
    }
}

impl<'de> Deserialize<'de> for AchievementRate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::from_ten_thousandths(value).map_err(de::Error::custom)
    }
}

/// 宴会场达成率使用与官服相同的万分之一百分比定点单位。
///
/// 宴谱不适用普通谱的 101.0000% 上限，因此保留完整 `u32` wire 值；
/// 普通 Rating 计算仍只接受 [`AchievementRate`]。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UtageScore(u32);

impl UtageScore {
    pub const fn from_ten_thousandths(value: u32) -> Self {
        Self(value)
    }

    pub fn from_decimal_str(value: &str) -> Result<Self, RatingError> {
        decimal_units(value).map(Self)
    }

    pub const fn ten_thousandths(self) -> u32 {
        self.0
    }

    pub fn decimal_string(self) -> String {
        decimal_achievement(self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayAchievementKind {
    Ranked,
    Utage,
}

impl PlayAchievementKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ranked => "ranked",
            Self::Utage => "utage",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlayAchievementError {
    UnsupportedOfficialLevel(u8),
    RankedOutOfRange,
}

impl fmt::Display for PlayAchievementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedOfficialLevel(level) => {
                write!(formatter, "官服成绩 level 不受支持：{level}")
            }
            Self::RankedOutOfRange => formatter.write_str("普通谱达成率超出 101.0000% 上限"),
        }
    }
}

impl Error for PlayAchievementError {}

/// 一次游玩成绩的达成率。
///
/// 为兼容既有 JSON，普通谱继续序列化为原来的整数单位；宴谱必须使用带 kind 的
/// 对象。反序列化裸整数时只按普通谱校验，绝不根据大于 101% 猜测为宴谱。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PlayAchievement {
    Ranked(AchievementRate),
    Utage(UtageScore),
}

impl PlayAchievement {
    pub fn from_official(level: u8, units: u32) -> Result<Self, PlayAchievementError> {
        match level {
            0..=4 => AchievementRate::from_ten_thousandths(units)
                .map(Self::Ranked)
                .map_err(|_| PlayAchievementError::RankedOutOfRange),
            10 => Ok(Self::Utage(UtageScore::from_ten_thousandths(units))),
            _ => Err(PlayAchievementError::UnsupportedOfficialLevel(level)),
        }
    }

    pub fn from_parts(kind: PlayAchievementKind, units: u32) -> Result<Self, RatingError> {
        match kind {
            PlayAchievementKind::Ranked => {
                AchievementRate::from_ten_thousandths(units).map(Self::Ranked)
            }
            PlayAchievementKind::Utage => Ok(Self::Utage(UtageScore::from_ten_thousandths(units))),
        }
    }

    pub fn from_decimal_str(kind: PlayAchievementKind, value: &str) -> Result<Self, RatingError> {
        match kind {
            PlayAchievementKind::Ranked => {
                AchievementRate::from_decimal_str(value).map(Self::Ranked)
            }
            PlayAchievementKind::Utage => UtageScore::from_decimal_str(value).map(Self::Utage),
        }
    }

    pub const fn kind(self) -> PlayAchievementKind {
        match self {
            Self::Ranked(_) => PlayAchievementKind::Ranked,
            Self::Utage(_) => PlayAchievementKind::Utage,
        }
    }

    pub const fn ten_thousandths(self) -> u32 {
        match self {
            Self::Ranked(value) => value.ten_thousandths(),
            Self::Utage(value) => value.ten_thousandths(),
        }
    }

    pub const fn ranked(self) -> Option<AchievementRate> {
        match self {
            Self::Ranked(value) => Some(value),
            Self::Utage(_) => None,
        }
    }

    pub const fn utage(self) -> Option<UtageScore> {
        match self {
            Self::Ranked(_) => None,
            Self::Utage(value) => Some(value),
        }
    }

    pub fn decimal_string(self) -> String {
        decimal_achievement(self.ten_thousandths())
    }
}

impl From<AchievementRate> for PlayAchievement {
    fn from(value: AchievementRate) -> Self {
        Self::Ranked(value)
    }
}

impl From<UtageScore> for PlayAchievement {
    fn from(value: UtageScore) -> Self {
        Self::Utage(value)
    }
}

#[derive(Serialize, Deserialize)]
struct TaggedPlayAchievement {
    kind: PlayAchievementKind,
    units: u32,
}

impl Serialize for PlayAchievement {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match *self {
            Self::Ranked(value) => value.serialize(serializer),
            Self::Utage(value) => TaggedPlayAchievement {
                kind: PlayAchievementKind::Utage,
                units: value.ten_thousandths(),
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PlayAchievement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Legacy(u32),
            Tagged(TaggedPlayAchievement),
        }

        match Wire::deserialize(deserializer)? {
            Wire::Legacy(units) => AchievementRate::from_ten_thousandths(units)
                .map(Self::Ranked)
                .map_err(de::Error::custom),
            Wire::Tagged(value) => {
                Self::from_parts(value.kind, value.units).map_err(de::Error::custom)
            }
        }
    }
}

fn parse_decimal(input: &str) -> Result<Decimal, RatingError> {
    let value = input.trim();
    let value = value.strip_prefix('+').unwrap_or(value);
    let parsed = value
        .parse::<Decimal>()
        .map_err(|_| RatingError::InvalidDecimal {
            field: "achievements",
            value: input.to_owned(),
        })?
        .normalize();
    if parsed.is_sign_negative() {
        return Err(RatingError::InvalidDecimal {
            field: "achievements",
            value: input.to_owned(),
        });
    }
    if parsed.scale() > 4 {
        return Err(RatingError::TooPrecise {
            field: "achievements",
            decimal_places: 4,
        });
    }
    Ok(parsed)
}

fn decimal_units(input: &str) -> Result<u32, RatingError> {
    (parse_decimal(input)? * Decimal::from(ACHIEVEMENT_SCALE))
        .to_u32()
        .ok_or(RatingError::OutOfRange {
            field: "achievements",
        })
}

fn decimal_achievement(units: u32) -> String {
    format!(
        "{}.{:04}",
        units / ACHIEVEMENT_SCALE,
        units % ACHIEVEMENT_SCALE
    )
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::{
        AchievementRate, PlayAchievement, PlayAchievementError, PlayAchievementKind, UtageScore,
    };
    use crate::RatingError;

    #[test]
    fn ranked_limit_stays_at_101_percent() {
        assert!(AchievementRate::from_decimal_str("101.0000").is_ok());
        assert_eq!(
            AchievementRate::from_decimal_str("101.0001"),
            Err(RatingError::OutOfRange {
                field: "achievements",
            })
        );
    }

    #[test]
    fn official_constructor_uses_level_instead_of_value_guessing() {
        assert_eq!(
            PlayAchievement::from_official(3, 1_010_000),
            AchievementRate::from_ten_thousandths(1_010_000)
                .map(PlayAchievement::Ranked)
                .map_err(|_| PlayAchievementError::RankedOutOfRange)
        );
        assert_eq!(
            PlayAchievement::from_official(3, 1_535_756),
            Err(PlayAchievementError::RankedOutOfRange)
        );
        assert_eq!(
            PlayAchievement::from_official(10, 1_535_756),
            Ok(PlayAchievement::Utage(UtageScore::from_ten_thousandths(
                1_535_756
            )))
        );
        assert_eq!(
            PlayAchievement::from_official(5, 1_000_000),
            Err(PlayAchievementError::UnsupportedOfficialLevel(5))
        );
    }

    #[test]
    fn serde_preserves_ranked_wire_and_tags_utage() -> Result<(), Box<dyn Error>> {
        let ranked = PlayAchievement::from(AchievementRate::from_decimal_str("101.0000")?);
        let utage = PlayAchievement::from(UtageScore::from_ten_thousandths(1_535_756));

        assert_eq!(serde_json::to_string(&ranked)?, "1010000");
        assert_eq!(
            serde_json::to_string(&utage)?,
            r#"{"kind":"utage","units":1535756}"#
        );
        assert_eq!(serde_json::from_str::<PlayAchievement>("1010000")?, ranked);
        assert_eq!(
            serde_json::from_str::<PlayAchievement>(r#"{"kind":"utage","units":1535756}"#)?,
            utage
        );
        assert_eq!(utage.decimal_string(), "153.5756");
        assert_eq!(utage.ranked(), None);
        assert_eq!(
            utage.utage(),
            Some(UtageScore::from_ten_thousandths(1_535_756))
        );
        assert_eq!(utage.kind(), PlayAchievementKind::Utage);
        assert!(serde_json::from_str::<PlayAchievement>("1535756").is_err());
        assert!(
            serde_json::from_str::<PlayAchievement>(r#"{"kind":"ranked","units":1010001}"#)
                .is_err()
        );
        Ok(())
    }
}
