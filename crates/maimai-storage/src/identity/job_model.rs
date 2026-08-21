use std::fmt;

use time::OffsetDateTime;

use super::IdentityStats;
use crate::StorageError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityJobStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl IdentityJobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub(super) fn from_stored(value: &str) -> Result<Self, StorageError> {
        match value {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(invalid("identity_job.status", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityRefreshReason {
    ForceRefresh,
    StaleOrMissing,
    AutoDaily,
}

impl IdentityRefreshReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ForceRefresh => "force_refresh",
            Self::StaleOrMissing => "stale_or_missing",
            Self::AutoDaily => "auto_daily",
        }
    }

    pub(super) fn from_stored(value: &str) -> Result<Self, StorageError> {
        match value {
            "force_refresh" => Ok(Self::ForceRefresh),
            "stale_or_missing" => Ok(Self::StaleOrMissing),
            "auto_daily" => Ok(Self::AutoDaily),
            _ => Err(invalid("identity_job.refresh_reason", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityJobErrorCode {
    InvalidInput,
    Timeout,
    Network,
    Http,
    NapCat,
    InvalidResponse,
    Storage,
    Unknown,
}

impl IdentityJobErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "INVALID_INPUT",
            Self::Timeout => "TIMEOUT",
            Self::Network => "NETWORK_ERROR",
            Self::Http => "HTTP_ERROR",
            Self::NapCat => "NAPCAT_ERROR",
            Self::InvalidResponse => "INVALID_JSON",
            Self::Storage => "STORAGE_ERROR",
            Self::Unknown => "UNKNOWN_ERROR",
        }
    }

    pub(super) fn from_stored(value: &str) -> Result<Self, StorageError> {
        match value {
            "INVALID_INPUT" => Ok(Self::InvalidInput),
            "TIMEOUT" => Ok(Self::Timeout),
            "NETWORK_ERROR" => Ok(Self::Network),
            "HTTP_ERROR" => Ok(Self::Http),
            "NAPCAT_ERROR" => Ok(Self::NapCat),
            "INVALID_JSON" => Ok(Self::InvalidResponse),
            "STORAGE_ERROR" => Ok(Self::Storage),
            "UNKNOWN_ERROR" => Ok(Self::Unknown),
            _ => Err(invalid("identity_job.error_code", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityJobError {
    pub code: IdentityJobErrorCode,
    pub message: String,
    pub status: Option<u16>,
    pub body: Option<String>,
}

impl fmt::Display for IdentityJobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IdentityJobProgress {
    pub processed_groups: u64,
    pub total_groups: Option<u64>,
    pub friend_count: Option<u64>,
    pub current_group_id: Option<String>,
    pub current_group_name: Option<String>,
    pub unique_users: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityJob {
    pub generation: u64,
    pub status: IdentityJobStatus,
    pub started_at: OffsetDateTime,
    pub finished_at: Option<OffsetDateTime>,
    pub refresh_reason: IdentityRefreshReason,
    pub message: String,
    pub progress: IdentityJobProgress,
    pub stats: Option<IdentityStats>,
    pub error: Option<IdentityJobError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityJobStart {
    Started(IdentityJob),
    AlreadyRunning(IdentityJob),
}

fn invalid(field: &'static str, value: impl Into<String>) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.into(),
    }
}
