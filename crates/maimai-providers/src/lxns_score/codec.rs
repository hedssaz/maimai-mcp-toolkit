use std::str::FromStr;

use maimai_core::{ChartConstant, PlayAchievement, PlayAchievementKind};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::{Number, Value};

pub(super) mod achievement_percent {
    use super::*;

    pub fn serialize<S>(value: &PlayAchievement, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Number::from_str(&value.decimal_string())
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

pub(super) mod optional_chart_constant {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<ChartConstant>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<Value>::deserialize(deserializer)?;
        value
            .map(|value| {
                decimal_text(value, "ds")
                    .and_then(|value| {
                        ChartConstant::from_decimal_str(&value).map_err(|error| error.to_string())
                    })
                    .map_err(de::Error::custom)
            })
            .transpose()
    }
}

pub(super) mod optional_legacy_dx_rating {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<Value>::deserialize(deserializer)?;
        Ok(match value {
            Some(Value::Number(value)) => {
                value.as_u64().and_then(|value| u32::try_from(value).ok())
            }
            Some(Value::String(value)) => value.trim().parse::<u32>().ok(),
            None | Some(Value::Null) | Some(_) => None,
        })
    }
}

pub(super) fn play_achievement(
    value: Value,
    kind: PlayAchievementKind,
) -> Result<PlayAchievement, String> {
    decimal_text(value, "achievements").and_then(|value| {
        PlayAchievement::from_decimal_str(kind, &value).map_err(|error| error.to_string())
    })
}

fn decimal_text(value: Value, field: &str) -> Result<String, String> {
    match value {
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value),
        _ => Err(format!("{field} must be a JSON number or decimal string")),
    }
}
