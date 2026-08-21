use std::fmt;

use serde::{Deserialize, Deserializer, de};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Friend {
    #[serde(alias = "userId", deserialize_with = "deserialize_qq_id")]
    user_id: String,
    #[serde(default, deserialize_with = "deserialize_optional_text")]
    nickname: Option<String>,
}

impl Friend {
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn nickname(&self) -> Option<&str> {
        self.nickname.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Group {
    #[serde(alias = "groupId", deserialize_with = "deserialize_group_id")]
    group_id: String,
    #[serde(
        default,
        alias = "groupName",
        deserialize_with = "deserialize_optional_text"
    )]
    group_name: Option<String>,
    #[serde(default, alias = "memberCount")]
    member_count: Option<u64>,
}

impl Group {
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    pub fn group_name(&self) -> Option<&str> {
        self.group_name.as_deref()
    }

    pub fn member_count(&self) -> Option<u64> {
        self.member_count
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct GroupMember {
    #[serde(
        default,
        alias = "groupId",
        deserialize_with = "deserialize_optional_group_id"
    )]
    pub(super) group_id: Option<String>,
    #[serde(alias = "userId", deserialize_with = "deserialize_qq_id")]
    user_id: String,
    #[serde(default, deserialize_with = "deserialize_optional_text")]
    nickname: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_text")]
    card: Option<String>,
}

impl GroupMember {
    pub fn group_id(&self) -> &str {
        self.group_id.as_deref().unwrap_or_default()
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn nickname(&self) -> Option<&str> {
        self.nickname.as_deref()
    }

    pub fn card(&self) -> Option<&str> {
        self.card.as_deref()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct OneBotEnvelope<T> {
    status: Option<String>,
    retcode: Option<i64>,
    message: Option<String>,
    pub(super) data: T,
}

impl<T> OneBotEnvelope<T> {
    pub(super) fn new(
        status: Option<String>,
        retcode: Option<i64>,
        message: Option<String>,
        data: T,
    ) -> Self {
        Self {
            status,
            retcode,
            message,
            data,
        }
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn retcode(&self) -> Option<i64> {
        self.retcode
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn data(&self) -> &T {
        &self.data
    }

    pub fn into_data(self) -> T {
        self.data
    }
}

impl<T> fmt::Debug for OneBotEnvelope<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OneBotEnvelope")
            .field("status", &self.status)
            .field("retcode", &self.retcode)
            .field("has_message", &self.message.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IdWire {
    Number(u64),
    Text(String),
}

fn deserialize_qq_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = normalized_id(IdWire::deserialize(deserializer)?)
        .ok_or_else(|| de::Error::custom("QQ ID 为空"))?;
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(de::Error::custom("QQ ID 必须是数字"));
    }
    Ok(value)
}

fn deserialize_group_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    normalized_id(IdWire::deserialize(deserializer)?).ok_or_else(|| de::Error::custom("群 ID 为空"))
}

fn deserialize_optional_group_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<IdWire>::deserialize(deserializer)?.map_or(Ok(None), |value| {
        normalized_id(value)
            .map(Some)
            .ok_or_else(|| de::Error::custom("群 ID 为空"))
    })
}

fn deserialize_optional_text<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty()))
}

fn normalized_id(value: IdWire) -> Option<String> {
    let value = match value {
        IdWire::Number(value) => value.to_string(),
        IdWire::Text(value) => value.trim().to_owned(),
    };
    (!value.is_empty() && !value.chars().any(char::is_control)).then_some(value)
}
