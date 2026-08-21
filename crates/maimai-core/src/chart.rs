use serde::{Deserialize, Deserializer, Serialize, de};

use crate::ValidationError;

/// 外部曲库的 ID 空间。相同数字在不同来源中不是同一个稳定标识。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SongIdNamespace {
    DivingFish,
    Lxns,
    OfficialCn,
    DxRating,
    Yuzu,
}

/// 有些日服或宴谱来源只有字符串 ID，不能强制解析成整数。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(untagged)]
pub enum SongIdValue {
    Numeric(u32),
    Text(SongIdText),
}

/// 已验证的文本歌曲 ID。字段保持私有，避免绕过统一的空值和控制字符校验。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SongIdText(String);

impl SongIdText {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if value.chars().any(char::is_control) {
            return Err(ValidationError::ControlCharacter { field: "songId" });
        }
        let value = value.trim().to_owned();
        if value.is_empty() {
            return Err(ValidationError::Empty { field: "songId" });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for SongIdText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SongIdValueWire {
    Numeric(u32),
    Text(String),
}

impl<'de> Deserialize<'de> for SongIdValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match SongIdValueWire::deserialize(deserializer)? {
            SongIdValueWire::Numeric(value) => Ok(Self::Numeric(value)),
            SongIdValueWire::Text(value) => Self::text(value).map_err(de::Error::custom),
        }
    }
}

impl SongIdValue {
    pub fn text(value: impl Into<String>) -> Result<Self, ValidationError> {
        SongIdText::new(value).map(Self::Text)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SourceSongId {
    namespace: SongIdNamespace,
    value: SongIdValue,
}

impl SourceSongId {
    pub const fn new(namespace: SongIdNamespace, value: SongIdValue) -> Self {
        Self { namespace, value }
    }

    pub const fn numeric(namespace: SongIdNamespace, value: u32) -> Self {
        Self::new(namespace, SongIdValue::Numeric(value))
    }

    pub fn text(
        namespace: SongIdNamespace,
        value: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self::new(namespace, SongIdValue::text(value)?))
    }

    pub const fn namespace(&self) -> SongIdNamespace {
        self.namespace
    }

    pub const fn value(&self) -> &SongIdValue {
        &self.value
    }

    pub fn into_parts(self) -> (SongIdNamespace, SongIdValue) {
        (self.namespace, self.value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartGeneration {
    Standard,
    Deluxe,
    UtageOnePlayer,
    UtageTwoPlayer,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Basic,
    Advanced,
    Expert,
    Master,
    ReMaster,
    Utage,
}

/// 成绩和谱面使用来源感知的复合键，不再以可变曲名或 `+10000` 猜测身份。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ChartKey {
    song: SourceSongId,
    generation: ChartGeneration,
    difficulty: Difficulty,
}

impl ChartKey {
    pub fn new(
        song: SourceSongId,
        generation: ChartGeneration,
        difficulty: Difficulty,
    ) -> Result<Self, ValidationError> {
        let is_utage_generation = matches!(
            generation,
            ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
        );
        if is_utage_generation != matches!(difficulty, Difficulty::Utage) {
            return Err(ValidationError::InvalidChartKey);
        }
        Ok(Self {
            song,
            generation,
            difficulty,
        })
    }

    pub const fn song(&self) -> &SourceSongId {
        &self.song
    }

    pub const fn generation(&self) -> ChartGeneration {
        self.generation
    }

    pub const fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    pub fn into_parts(self) -> (SourceSongId, ChartGeneration, Difficulty) {
        (self.song, self.generation, self.difficulty)
    }
}

#[derive(Deserialize)]
struct ChartKeyWire {
    song: SourceSongId,
    generation: ChartGeneration,
    difficulty: Difficulty,
}

impl<'de> Deserialize<'de> for ChartKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ChartKeyWire::deserialize(deserializer)?;
        Self::new(wire.song, wire.generation, wire.difficulty).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChartGeneration, ChartKey, Difficulty, SongIdNamespace, SongIdText, SongIdValue,
        SourceSongId,
    };

    #[test]
    fn same_numeric_id_in_different_namespaces_is_distinct() -> Result<(), crate::ValidationError> {
        let diving_fish = ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::DivingFish, 383),
            ChartGeneration::Standard,
            Difficulty::Master,
        )?;
        let lxns = ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::Lxns, 383),
            ChartGeneration::Standard,
            Difficulty::Master,
        )?;
        assert_ne!(diving_fish, lxns);
        Ok(())
    }

    #[test]
    fn song_id_serde_preserves_untagged_shape_and_validates_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let numeric: SongIdValue = serde_json::from_str("383")?;
        let text: SongIdValue = serde_json::from_str(r#""宴-一人""#)?;

        assert_eq!(numeric, SongIdValue::Numeric(383));
        assert_eq!(text, SongIdValue::text("宴-一人")?);
        assert_eq!(serde_json::to_string(&numeric)?, "383");
        assert_eq!(serde_json::to_string(&text)?, r#""宴-一人""#);
        assert!(serde_json::from_str::<SongIdValue>(r#""""#).is_err());
        assert!(serde_json::from_str::<SongIdValue>(r#""   ""#).is_err());
        assert!(SongIdValue::text("song\nvariant").is_err());
        assert!(SongIdValue::text("\nsong").is_err());
        assert!(serde_json::from_str::<SongIdValue>(r#""song\nvariant""#).is_err());
        assert!(serde_json::from_str::<SongIdText>(r#""\t""#).is_err());

        let typed = SongIdText::new("  宴-二人  ")?;
        assert_eq!(typed.as_str(), "宴-二人");
        assert_eq!(serde_json::to_string(&typed)?, r#""宴-二人""#);
        assert_eq!(typed.into_string(), "宴-二人");
        Ok(())
    }

    #[test]
    fn source_song_id_serde_preserves_object_shape() -> Result<(), Box<dyn std::error::Error>> {
        let id = SourceSongId::text(SongIdNamespace::Yuzu, "宴-一人")?;
        let encoded = serde_json::to_value(&id)?;

        assert_eq!(
            encoded,
            serde_json::json!({"namespace": "yuzu", "value": "宴-一人"})
        );
        assert_eq!(serde_json::from_value::<SourceSongId>(encoded)?, id);
        assert!(
            serde_json::from_value::<SourceSongId>(
                serde_json::json!({"namespace": "yuzu", "value": "song\nvariant"})
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn chart_key_serde_preserves_shape_and_rejects_invalid_combinations()
    -> Result<(), Box<dyn std::error::Error>> {
        let key = ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::DivingFish, 383),
            ChartGeneration::Deluxe,
            Difficulty::Master,
        )?;
        let encoded = serde_json::to_value(&key)?;

        assert_eq!(
            encoded,
            serde_json::json!({
                "song": {"namespace": "diving_fish", "value": 383},
                "generation": "deluxe",
                "difficulty": "master"
            })
        );
        assert_eq!(serde_json::from_value::<ChartKey>(encoded)?, key);

        for invalid in [
            (ChartGeneration::Standard, Difficulty::Utage),
            (ChartGeneration::Deluxe, Difficulty::Utage),
            (ChartGeneration::UtageOnePlayer, Difficulty::Master),
            (ChartGeneration::UtageTwoPlayer, Difficulty::ReMaster),
        ] {
            assert!(
                ChartKey::new(
                    SourceSongId::numeric(SongIdNamespace::DivingFish, 383),
                    invalid.0,
                    invalid.1,
                )
                .is_err()
            );
            assert!(
                serde_json::from_value::<ChartKey>(serde_json::json!({
                    "song": {"namespace": "diving_fish", "value": 383},
                    "generation": invalid.0,
                    "difficulty": invalid.1
                }))
                .is_err()
            );
        }

        for valid in [
            ChartGeneration::UtageOnePlayer,
            ChartGeneration::UtageTwoPlayer,
        ] {
            ChartKey::new(
                SourceSongId::numeric(SongIdNamespace::Yuzu, 1),
                valid,
                Difficulty::Utage,
            )?;
        }
        Ok(())
    }
}
