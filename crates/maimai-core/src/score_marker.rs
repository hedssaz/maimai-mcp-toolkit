use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FullComboStatus {
    FullCombo,
    FullComboPlus,
    AllPerfect,
    AllPerfectPlus,
}

impl FullComboStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullCombo => "fc",
            Self::FullComboPlus => "fcp",
            Self::AllPerfect => "ap",
            Self::AllPerfectPlus => "app",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FullSyncStatus {
    Sync,
    FullSync,
    FullSyncPlus,
    FullSyncDeluxe,
    FullSyncDeluxePlus,
}

impl FullSyncStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::FullSync => "fs",
            Self::FullSyncPlus => "fsp",
            Self::FullSyncDeluxe => "fsd",
            Self::FullSyncDeluxePlus => "fsdp",
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("未知的 {field} 状态: {value}")]
pub struct ScoreMarkerParseError {
    field: &'static str,
    value: String,
}

impl ScoreMarkerParseError {
    pub const fn field(&self) -> &'static str {
        self.field
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn new(field: &'static str, value: &str) -> Self {
        Self {
            field,
            value: value.to_owned(),
        }
    }
}

impl FromStr for FullComboStatus {
    type Err = ScoreMarkerParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fc" => Ok(Self::FullCombo),
            "fcp" | "fc+" => Ok(Self::FullComboPlus),
            "ap" => Ok(Self::AllPerfect),
            "app" | "ap+" => Ok(Self::AllPerfectPlus),
            _ => Err(ScoreMarkerParseError::new("fc", value)),
        }
    }
}

impl FromStr for FullSyncStatus {
    type Err = ScoreMarkerParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sync" => Ok(Self::Sync),
            "fs" => Ok(Self::FullSync),
            "fsp" | "fs+" => Ok(Self::FullSyncPlus),
            "fsd" | "fdx" => Ok(Self::FullSyncDeluxe),
            "fsdp" | "fdxp" | "fdx+" | "fsd+" => Ok(Self::FullSyncDeluxePlus),
            _ => Err(ScoreMarkerParseError::new("fs", value)),
        }
    }
}

macro_rules! marker_wire {
    ($type:ty) => {
        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl AsRef<str> for $type {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(de::Error::custom)
            }
        }
    };
}

marker_wire!(FullComboStatus);
marker_wire!(FullSyncStatus);

#[cfg(test)]
mod tests {
    use super::{FullComboStatus, FullSyncStatus};

    #[test]
    fn aliases_parse_but_wire_values_are_canonical() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            " FC+ ".parse::<FullComboStatus>()?,
            FullComboStatus::FullComboPlus
        );
        assert_eq!(
            "AP+".parse::<FullComboStatus>()?,
            FullComboStatus::AllPerfectPlus
        );
        assert_eq!(
            " FDX+ ".parse::<FullSyncStatus>()?,
            FullSyncStatus::FullSyncDeluxePlus
        );
        assert_eq!("SYNC".parse::<FullSyncStatus>()?, FullSyncStatus::Sync);
        assert_eq!(
            serde_json::to_string(&FullComboStatus::FullComboPlus)?,
            r#""fcp""#
        );
        assert_eq!(
            serde_json::to_string(&FullSyncStatus::FullSyncDeluxePlus)?,
            r#""fsdp""#
        );
        Ok(())
    }

    #[test]
    fn unknown_marker_is_rejected_with_field_and_value() {
        let error = "clear".parse::<FullComboStatus>().err();
        assert_eq!(error.as_ref().map(|value| value.field()), Some("fc"));
        assert_eq!(error.as_ref().map(|value| value.value()), Some("clear"));
        assert!(serde_json::from_str::<FullSyncStatus>(r#""unknown""#).is_err());
    }
}
