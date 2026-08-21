use std::fmt;

use maimai_core::{QqId, ScoreSource};
use secrecy::SecretString;
use time::OffsetDateTime;

use super::{ScoreSettingsError, ScoreSettingsErrorCode};

pub const DEVELOPER_TOKEN_SECURITY_NOTICE: &str =
    "Developer-Token 只保存在本地，不会在状态结果中返回明文。";

pub struct DeveloperToken(SecretString);

impl DeveloperToken {
    pub fn new(value: impl Into<String>) -> Result<Self, ScoreSettingsError> {
        let value = value.into().trim().to_owned();
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(ScoreSettingsError::public(
                ScoreSettingsErrorCode::InvalidInput,
                "必须提供 developerToken。",
            ));
        }
        Ok(Self(SecretString::from(value)))
    }

    pub(super) fn secret(&self) -> &SecretString {
        &self.0
    }
}

impl fmt::Debug for DeveloperToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeveloperToken([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeveloperTokenStatus {
    pub bound: bool,
    pub updated_at: Option<OffsetDateTime>,
    pub security_notice: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClearDeveloperTokenResult {
    pub bound: bool,
    pub cleared: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreSourceSetting {
    pub qq: QqId,
    pub source: ScoreSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllowedScoreSources {
    diving_fish: bool,
    lxns: bool,
    local: bool,
}

impl AllowedScoreSources {
    pub const fn main() -> Self {
        Self {
            diving_fish: true,
            lxns: true,
            local: true,
        }
    }

    pub const fn public() -> Self {
        Self {
            diving_fish: true,
            lxns: false,
            local: true,
        }
    }

    pub const fn allows(self, source: ScoreSource) -> bool {
        match source {
            ScoreSource::DivingFish => self.diving_fish,
            ScoreSource::Lxns => self.lxns,
            ScoreSource::Local => self.local,
            ScoreSource::OfficialCn => false,
        }
    }
}
