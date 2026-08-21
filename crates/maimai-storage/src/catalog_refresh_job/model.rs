use time::OffsetDateTime;

use crate::StorageError;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CatalogRefreshJobId(i64);

impl CatalogRefreshJobId {
    pub fn parse(value: &str) -> Result<Self, StorageError> {
        let id = value
            .parse::<i64>()
            .map_err(|_| invalid("catalog_refresh_job.id", value))?;
        if id < 1 {
            return Err(invalid("catalog_refresh_job.id", value));
        }
        Ok(Self(id))
    }

    pub const fn value(self) -> i64 {
        self.0
    }

    pub(crate) fn from_stored(value: i64) -> Result<Self, StorageError> {
        if value < 1 {
            return Err(invalid("catalog_refresh_job.id", value.to_string()));
        }
        Ok(Self(value))
    }
}

impl std::fmt::Display for CatalogRefreshJobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogRefreshJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl CatalogRefreshJobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Interrupted)
    }

    pub(crate) fn from_stored(value: &str) -> Result<Self, StorageError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(invalid("catalog_refresh_job.status", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogRefreshJobOutcome {
    Success,
    PartialFailure,
    Failed,
    Interrupted,
}

impl CatalogRefreshJobOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::PartialFailure => "partial_failure",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub(crate) fn from_stored(value: &str) -> Result<Self, StorageError> {
        match value {
            "success" => Ok(Self::Success),
            "partial_failure" => Ok(Self::PartialFailure),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(invalid("catalog_refresh_job.outcome", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogRefreshSourceStatus {
    Queued,
    Pending,
    Updated,
    NotModified,
    Skipped,
    Failed,
    DiskUpdatedPendingReload,
}

impl CatalogRefreshSourceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Pending => "pending",
            Self::Updated => "updated",
            Self::NotModified => "not_modified",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::DiskUpdatedPendingReload => "disk_updated_pending_reload",
        }
    }

    pub const fn succeeded(self) -> bool {
        matches!(self, Self::Updated | Self::NotModified | Self::Skipped)
    }

    pub const fn completed(self) -> bool {
        matches!(
            self,
            Self::Updated
                | Self::NotModified
                | Self::Skipped
                | Self::Failed
                | Self::DiskUpdatedPendingReload
        )
    }

    pub(crate) fn from_stored(value: &str) -> Result<Self, StorageError> {
        match value {
            "queued" => Ok(Self::Queued),
            "pending" => Ok(Self::Pending),
            "updated" => Ok(Self::Updated),
            "not_modified" => Ok(Self::NotModified),
            "skipped" => Ok(Self::Skipped),
            "failed" => Ok(Self::Failed),
            "disk_updated_pending_reload" => Ok(Self::DiskUpdatedPendingReload),
            _ => Err(invalid("catalog_refresh_job_source.status", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogRefreshJobSource {
    pub position: u32,
    pub source: String,
    pub due: bool,
    pub status: CatalogRefreshSourceStatus,
    pub duration_millis: Option<u64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogRefreshJob {
    pub id: CatalogRefreshJobId,
    pub status: CatalogRefreshJobStatus,
    pub outcome: Option<CatalogRefreshJobOutcome>,
    pub created_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
    pub finished_at: Option<OffsetDateTime>,
    pub message: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub sources: Vec<CatalogRefreshJobSource>,
}

impl CatalogRefreshJob {
    pub fn completed_sources(&self) -> usize {
        self.sources
            .iter()
            .filter(|source| source.status.completed())
            .count()
    }

    pub fn succeeded_sources(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|source| source.status.succeeded())
            .map(|source| source.source.as_str())
            .collect()
    }

    pub fn failed_sources(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|source| {
                matches!(
                    source.status,
                    CatalogRefreshSourceStatus::Failed
                        | CatalogRefreshSourceStatus::DiskUpdatedPendingReload
                )
            })
            .map(|source| source.source.as_str())
            .collect()
    }

    pub fn skipped_sources(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|source| source.status == CatalogRefreshSourceStatus::Skipped)
            .map(|source| source.source.as_str())
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogRefreshJobStart {
    Started(CatalogRefreshJob),
    AlreadyRunning(CatalogRefreshJob),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogRefreshSourceUpdate {
    pub source: String,
    pub status: CatalogRefreshSourceStatus,
    pub duration_millis: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogRefreshTerminalUpdate {
    pub status: CatalogRefreshJobStatus,
    pub outcome: CatalogRefreshJobOutcome,
    pub message: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

fn invalid(field: &'static str, value: impl Into<String>) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.into(),
    }
}
