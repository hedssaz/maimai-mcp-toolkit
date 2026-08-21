use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::ValidationError;

/// QQ 标识按字符串保存，避免丢失前导零或被浮点 JSON 改写。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct QqId(String);

impl QqId {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into().trim().to_owned();
        if value.is_empty() {
            return Err(ValidationError::Empty { field: "QQ" });
        }
        if !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ValidationError::NonNumeric { field: "QQ" });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for QqId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for QqId {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for QqId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// 群或会话标识。部分适配器使用非数字会话 ID，因此只约束非空。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GroupId(String);

impl GroupId {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if value.chars().any(char::is_control) {
            return Err(ValidationError::ControlCharacter { field: "groupId" });
        }
        let value = value.trim().to_owned();
        if value.is_empty() {
            return Err(ValidationError::Empty { field: "groupId" });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for GroupId {
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
    use super::{GroupId, QqId};
    use crate::ValidationError;

    #[test]
    fn qq_keeps_its_string_identity() -> Result<(), ValidationError> {
        let qq = QqId::new(" 001234 ")?;
        assert_eq!(qq.as_str(), "001234");
        Ok(())
    }

    #[test]
    fn qq_rejects_non_digits() {
        assert_eq!(
            QqId::new("123x"),
            Err(ValidationError::NonNumeric { field: "QQ" })
        );
    }

    #[test]
    fn group_accepts_adapter_scoped_ids() -> Result<(), ValidationError> {
        let group = GroupId::new("qq:987654")?;
        assert_eq!(group.as_str(), "qq:987654");
        Ok(())
    }

    #[test]
    fn serde_uses_identity_constructors() -> Result<(), serde_json::Error> {
        let qq: QqId = serde_json::from_str(r#"" 001234 ""#)?;
        let group: GroupId = serde_json::from_str(r#"" qq:987654 ""#)?;

        assert_eq!(qq.as_str(), "001234");
        assert_eq!(group.as_str(), "qq:987654");
        assert_eq!(serde_json::to_string(&qq)?, r#""001234""#);
        assert!(serde_json::from_str::<QqId>(r#""123x""#).is_err());
        assert!(serde_json::from_str::<QqId>(r#""   ""#).is_err());
        assert!(serde_json::from_str::<GroupId>(r#""   ""#).is_err());
        assert!(GroupId::new("group\nname").is_err());
        assert!(GroupId::new("\ngroup").is_err());
        assert!(serde_json::from_str::<GroupId>(r#""group\nname""#).is_err());
        Ok(())
    }
}
