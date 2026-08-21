use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{QqId, ValidationError};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PlayerUsername(String);

impl PlayerUsername {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if value.chars().any(char::is_control) {
            return Err(ValidationError::ControlCharacter { field: "username" });
        }
        let value = value.trim().to_owned();
        if value.is_empty() {
            return Err(ValidationError::Empty { field: "username" });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlayerUsername {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PlayerUsername {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{PlayerSelector, PlayerUsername};

    #[test]
    fn serde_uses_username_constructor() -> Result<(), serde_json::Error> {
        let username: PlayerUsername = serde_json::from_str(r#"" Alice ""#)?;
        assert_eq!(username.as_str(), "Alice");
        assert_eq!(serde_json::to_string(&username)?, r#""Alice""#);
        assert!(serde_json::from_str::<PlayerUsername>(r#""   ""#).is_err());
        assert!(PlayerUsername::new("name\0tail").is_err());
        assert!(PlayerUsername::new("name\n").is_err());
        assert!(serde_json::from_str::<PlayerUsername>(r#""name\u0000tail""#).is_err());
        Ok(())
    }

    #[test]
    fn auto_selector_keeps_tag_shape_and_rejects_empty_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"{"kind":"auto","value":" Alice "}"#;
        let selector: PlayerSelector = serde_json::from_str(source)?;

        assert_eq!(
            selector,
            PlayerSelector::Auto(PlayerUsername::new("Alice")?)
        );
        assert_eq!(
            serde_json::to_string(&selector)?,
            r#"{"kind":"auto","value":"Alice"}"#
        );
        assert!(
            serde_json::from_str::<PlayerSelector>(r#"{"kind":"auto","value":"   "}"#).is_err()
        );
        Ok(())
    }
}

/// 保留旧 `target` 的“身份缓存命中后按 QQ，否则按用户名”语义。
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum PlayerSelector {
    Qq(QqId),
    Username(PlayerUsername),
    Auto(PlayerUsername),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreSource {
    DivingFish,
    Lxns,
    Local,
    OfficialCn,
}
