use std::{fmt, num::NonZeroU32};

use maimai_core::{ChartConstant, PlayAchievement, PlayAchievementKind};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeStruct};
use serde_json::Value;

use super::{LxnsScoreError, LxnsScoreErrorCode};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FriendCode(String);

impl FriendCode {
    pub fn new(value: impl AsRef<str>) -> Result<Self, LxnsScoreError> {
        let normalized = value
            .as_ref()
            .chars()
            .filter(|character| !character.is_whitespace() && !matches!(character, '_' | '-'))
            .collect::<String>();
        if !(6..=18).contains(&normalized.len())
            || !normalized.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid("LXNS friend code 必须是 6 到 18 位数字"));
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FriendCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for FriendCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0
            .parse::<u64>()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FriendCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match StringOrU64::deserialize(deserializer)? {
            StringOrU64::String(value) => Self::new(value).map_err(de::Error::custom),
            StringOrU64::Number(value) => Self::new(value.to_string()).map_err(de::Error::custom),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LxnsSongId(NonZeroU32);

impl LxnsSongId {
    pub fn new(value: u32) -> Result<Self, LxnsScoreError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or_else(|| invalid("LXNS song id 必须大于零"))
    }

    pub fn for_query(value: u32) -> Result<Self, LxnsScoreError> {
        let normalized = if (10_001..100_000).contains(&value) {
            value % 10_000
        } else {
            value
        };
        Self::new(normalized)
    }

    pub fn from_waterfish(value: u32, chart_type: LxnsChartType) -> Result<Self, LxnsScoreError> {
        let normalized = if value > 100_000 {
            value
        } else if matches!(chart_type, LxnsChartType::Standard | LxnsChartType::Deluxe)
            && value > 10_000
        {
            value % 10_000
        } else {
            value
        };
        Self::new(normalized)
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LxnsChartType {
    Standard,
    Deluxe,
    Utage,
}

impl LxnsChartType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Deluxe => "dx",
            Self::Utage => "utage",
        }
    }

    pub fn from_waterfish(value: &str, song_id: u32) -> Result<Self, LxnsScoreError> {
        if song_id > 100_000 {
            return Ok(Self::Utage);
        }
        match value.trim().to_ascii_lowercase().as_str() {
            "utage" | "宴" => Ok(Self::Utage),
            "dx" => Ok(Self::Deluxe),
            "sd" | "standard" | "st" => Ok(Self::Standard),
            _ => Err(invalid("LXNS chart type 不受支持")),
        }
    }
}

impl Serialize for LxnsChartType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LxnsChartType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.trim().to_ascii_lowercase().as_str() {
            "standard" | "sd" | "st" => Ok(Self::Standard),
            "dx" | "deluxe" => Ok(Self::Deluxe),
            "utage" | "宴" => Ok(Self::Utage),
            _ => Err(de::Error::custom("invalid LXNS chart type")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LxnsDifficulty {
    Basic,
    Advanced,
    Expert,
    Master,
    ReMaster,
}

impl LxnsDifficulty {
    pub const fn index(self) -> u8 {
        match self {
            Self::Basic => 0,
            Self::Advanced => 1,
            Self::Expert => 2,
            Self::Master => 3,
            Self::ReMaster => 4,
        }
    }

    fn from_index(value: u8) -> Result<Self, &'static str> {
        match value {
            0 => Ok(Self::Basic),
            1 => Ok(Self::Advanced),
            2 => Ok(Self::Expert),
            3 => Ok(Self::Master),
            4 => Ok(Self::ReMaster),
            _ => Err("invalid LXNS level_index"),
        }
    }
}

impl Serialize for LxnsDifficulty {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.index())
    }
}

impl<'de> Deserialize<'de> for LxnsDifficulty {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_index(u8::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FullCombo {
    Fc,
    Fcp,
    Ap,
    App,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FullSync {
    Sync,
    Fs,
    Fsp,
    Fsd,
    Fsdp,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct CollectionRef {
    id: NonZeroU32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

impl Serialize for CollectionRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("CollectionRef", 1)?;
        state.serialize_field("id", &self.id)?;
        state.end()
    }
}

impl CollectionRef {
    pub fn new(id: u32) -> Result<Self, LxnsScoreError> {
        NonZeroU32::new(id)
            .map(|id| Self {
                id,
                name: None,
                color: None,
            })
            .ok_or_else(|| invalid("LXNS collection id 必须大于零"))
    }

    pub const fn get(&self) -> u32 {
        self.id.get()
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
pub struct LxnsPlayer {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, alias = "friendCode", alias = "userFriendCode")]
    pub friend_code: Option<FriendCode>,
    #[serde(default)]
    pub rating: Option<u32>,
    #[serde(default, alias = "courseRank")]
    pub course_rank: Option<u32>,
    #[serde(default, alias = "classRank")]
    pub class_rank: Option<u32>,
    #[serde(default)]
    pub star: Option<u32>,
    #[serde(default)]
    pub trophy: Option<CollectionRef>,
    #[serde(default)]
    pub icon: Option<CollectionRef>,
    #[serde(default, alias = "namePlate")]
    pub name_plate: Option<CollectionRef>,
    #[serde(default)]
    pub frame: Option<CollectionRef>,
    #[serde(default, alias = "uploadTime")]
    pub upload_time: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LxnsScore {
    pub id: LxnsSongId,
    pub chart_type: LxnsChartType,
    pub level_index: LxnsDifficulty,
    pub achievements: PlayAchievement,
    pub dx_score: u32,
    pub dx_rating: Option<u32>,
    pub song_name: Option<String>,
    pub title: Option<String>,
    pub level: Option<String>,
    pub ds: Option<ChartConstant>,
    pub fc: Option<FullCombo>,
    pub fs: Option<FullSync>,
    pub rate: Option<String>,
}

#[derive(Deserialize)]
struct LxnsScoreWire {
    #[serde(alias = "song_id", alias = "songId")]
    id: LxnsSongId,
    #[serde(rename = "type", alias = "chart_type", alias = "chartType")]
    chart_type: LxnsChartType,
    #[serde(alias = "levelIndex")]
    level_index: LxnsDifficulty,
    achievements: Value,
    #[serde(default, alias = "dxScore")]
    dx_score: u32,
    #[serde(default, alias = "dxRating")]
    dx_rating: Option<u32>,
    #[serde(default, alias = "songName")]
    song_name: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default, with = "super::codec::optional_chart_constant")]
    ds: Option<ChartConstant>,
    #[serde(default)]
    fc: Option<FullCombo>,
    #[serde(default)]
    fs: Option<FullSync>,
    #[serde(default)]
    rate: Option<String>,
}

impl<'de> Deserialize<'de> for LxnsScore {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LxnsScoreWire::deserialize(deserializer)?;
        let kind = if wire.chart_type == LxnsChartType::Utage {
            PlayAchievementKind::Utage
        } else {
            PlayAchievementKind::Ranked
        };
        Ok(Self {
            id: wire.id,
            chart_type: wire.chart_type,
            level_index: wire.level_index,
            achievements: super::codec::play_achievement(wire.achievements, kind)
                .map_err(de::Error::custom)?,
            dx_score: wire.dx_score,
            dx_rating: wire.dx_rating,
            song_name: wire.song_name,
            title: wire.title,
            level: wire.level,
            ds: wire.ds,
            fc: wire.fc,
            fs: wire.fs,
            rate: wire.rate,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LxnsPlayerScores {
    pub player: Option<LxnsPlayer>,
    pub scores: Vec<LxnsScore>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LxnsPlayerBests {
    pub player: Option<LxnsPlayer>,
    pub standard: Vec<LxnsScore>,
    pub deluxe: Vec<LxnsScore>,
    pub standard_total: Option<u32>,
    pub deluxe_total: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LxnsSongBests {
    pub song_id: LxnsSongId,
    pub player: Option<LxnsPlayer>,
    pub scores: Vec<LxnsScore>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrU64 {
    String(String),
    Number(u64),
}

fn invalid(message: &'static str) -> LxnsScoreError {
    LxnsScoreError::new(LxnsScoreErrorCode::InvalidRequest, message)
}
