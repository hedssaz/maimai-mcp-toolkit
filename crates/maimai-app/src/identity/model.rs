use std::{num::NonZeroUsize, time::Duration};

use maimai_core::{GroupId, PlayerUsername};
use maimai_storage::{IdentityJob, IdentityMetadata, IdentityRecord, IdentityStats};
use time::{OffsetDateTime, Time};

use super::IdentityError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoCache(bool);

impl NoCache {
    pub const fn new(value: bool) -> Self {
        Self(value)
    }

    pub const fn get(self) -> bool {
        self.0
    }
}

impl Default for NoCache {
    fn default() -> Self {
        Self(true)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupDelay(Duration);

impl GroupDelay {
    pub const DEFAULT: Self = Self(Duration::from_millis(250));
    pub const MAX: Duration = Duration::from_secs(10);

    pub fn new(value: Duration) -> Result<Self, IdentityError> {
        if value > Self::MAX {
            return Err(IdentityError::InvalidParameter {
                field: "group_delay",
            });
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> Duration {
        self.0
    }
}

impl Default for GroupDelay {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaxGroups(NonZeroUsize);

impl MaxGroups {
    pub fn new(value: usize) -> Result<Self, IdentityError> {
        NonZeroUsize::new(value)
            .map(Self)
            .ok_or(IdentityError::InvalidParameter {
                field: "max_groups",
            })
    }

    pub const fn get(self) -> usize {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RefreshOptions {
    pub no_cache: NoCache,
    pub group_delay: GroupDelay,
    pub max_groups: Option<MaxGroups>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefreshReport {
    pub performed: bool,
    pub metadata: IdentityMetadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshPolicy {
    Force,
    IfStale,
    AutoDaily,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefreshJobRequest {
    pub policy: RefreshPolicy,
    pub options: RefreshOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefreshJobLaunch {
    pub started: bool,
    pub cache: CacheStatus,
    pub job: Option<IdentityJob>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResetHour(Time);

impl ResetHour {
    pub fn new(value: u8) -> Result<Self, IdentityError> {
        Time::from_hms(value, 0, 0)
            .map(Self)
            .map_err(|_| IdentityError::InvalidParameter {
                field: "reset_hour_utc",
            })
    }

    pub const fn get(self) -> u8 {
        self.0.hour()
    }

    pub(crate) const fn time(self) -> Time {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheStatus {
    pub exists: bool,
    pub fresh: bool,
    pub age_seconds: Option<i64>,
    pub reset_hour_utc: ResetHour,
    pub fetched_at: Option<OffsetDateTime>,
    pub updated_at: Option<OffsetDateTime>,
    pub generation: Option<u64>,
    pub stats: IdentityStats,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityQuery(PlayerUsername);

impl IdentityQuery {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        PlayerUsername::new(value)
            .map(Self)
            .map_err(|_| IdentityError::InvalidParameter { field: "query" })
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaxResults(usize);

impl MaxResults {
    pub const DEFAULT: usize = 10;
    pub const MAX: usize = 20;

    pub fn new(value: usize) -> Result<Self, IdentityError> {
        if value > Self::MAX {
            return Err(IdentityError::InvalidParameter {
                field: "max_results",
            });
        }
        if value == 0 {
            return Err(IdentityError::InvalidParameter {
                field: "max_results",
            });
        }
        Ok(Self(value))
    }

    pub fn default_value() -> Self {
        Self(Self::DEFAULT)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IdentityField {
    Qq,
    QqNickname,
    FriendNickname,
    WaterfishNickname,
    WaterfishUsername,
    PreferredGroupNickname,
    PreferredGroupCard,
    PreferredGroupQqNickname,
    GroupNickname,
    GroupCard,
    GroupQqNickname,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityMatch {
    pub identity: IdentityRecord,
    pub score: u16,
    pub matched_fields: Vec<IdentityField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution {
    pub query: String,
    pub group_id: Option<GroupId>,
    pub matches: Vec<IdentityMatch>,
    pub ambiguous: bool,
}
