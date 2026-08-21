use std::time::{Duration, SystemTime};

use maimai_providers::{CatalogSource, SourceTarget};

use super::error::RefreshError;

pub const DEFAULT_TTL_DAYS: f64 = 30.0 / 1_440.0;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnabledSources(Vec<CatalogSource>);

impl EnabledSources {
    pub fn new(sources: impl IntoIterator<Item = CatalogSource>) -> Result<Self, RefreshError> {
        let mut enabled = Vec::new();
        for source in sources {
            if !enabled.contains(&source) {
                enabled.push(source);
            }
        }
        if enabled.is_empty() {
            return Err(RefreshError::InvalidSource {
                catalog_source: "<empty>",
            });
        }
        Ok(Self(enabled))
    }

    pub fn main() -> Self {
        Self(CatalogSource::ALL.to_vec())
    }

    pub fn public() -> Self {
        Self(vec![
            CatalogSource::Lxns,
            CatalogSource::DivingFish,
            CatalogSource::Yuzu,
            CatalogSource::ChartStats,
            CatalogSource::Plate,
        ])
    }

    pub fn contains(&self, source: CatalogSource) -> bool {
        self.0.contains(&source)
    }

    pub fn sources(&self) -> &[CatalogSource] {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub struct RefreshRequest {
    sources: Vec<CatalogSource>,
    ttl_days: f64,
    force: bool,
    check_only: bool,
    timeout: Duration,
}

impl RefreshRequest {
    pub fn new(
        sources: Vec<CatalogSource>,
        ttl_days: f64,
        force: bool,
        check_only: bool,
        timeout: Duration,
    ) -> Result<Self, RefreshError> {
        if sources.is_empty() {
            return Err(RefreshError::InvalidSource {
                catalog_source: "<empty>",
            });
        }
        if !ttl_days.is_finite() || ttl_days < 0.0 {
            return Err(RefreshError::InvalidTtl);
        }
        if timeout.is_zero() {
            return Err(RefreshError::InvalidTimeout);
        }
        let mut normalized = Vec::new();
        for source in sources {
            if !normalized.contains(&source) {
                normalized.push(source);
            }
        }
        Ok(Self {
            sources: normalized,
            ttl_days,
            force,
            check_only,
            timeout,
        })
    }

    pub fn sources(&self) -> &[CatalogSource] {
        &self.sources
    }

    pub const fn ttl_days(&self) -> f64 {
        self.ttl_days
    }

    pub const fn force(&self) -> bool {
        self.force
    }

    pub const fn check_only(&self) -> bool {
        self.check_only
    }

    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn check_only_copy(&self) -> Self {
        let mut request = self.clone();
        request.check_only = true;
        request
    }
}

#[derive(Clone, Debug)]
pub struct TargetStatus {
    pub(crate) target: SourceTarget,
    pub(crate) exists: bool,
    pub(crate) modified: Option<SystemTime>,
    pub(crate) age: Option<Duration>,
    pub(crate) expired: bool,
}

impl TargetStatus {
    pub const fn target(&self) -> SourceTarget {
        self.target
    }
    pub const fn exists(&self) -> bool {
        self.exists
    }
    pub const fn modified(&self) -> Option<SystemTime> {
        self.modified
    }
    pub const fn age(&self) -> Option<Duration> {
        self.age
    }
    pub const fn expired(&self) -> bool {
        self.expired
    }
}

#[derive(Clone, Debug)]
pub struct SourceStatus {
    pub(crate) source: CatalogSource,
    pub(crate) targets: Vec<TargetStatus>,
    pub(crate) oldest_modified: Option<SystemTime>,
    pub(crate) age: Option<Duration>,
    pub(crate) ttl_days: f64,
    pub(crate) expired: bool,
}

impl SourceStatus {
    pub const fn source(&self) -> CatalogSource {
        self.source
    }
    pub fn targets(&self) -> &[TargetStatus] {
        &self.targets
    }
    pub const fn oldest_modified(&self) -> Option<SystemTime> {
        self.oldest_modified
    }
    pub const fn age(&self) -> Option<Duration> {
        self.age
    }
    pub const fn ttl_days(&self) -> f64 {
        self.ttl_days
    }
    pub const fn expired(&self) -> bool {
        self.expired
    }
    pub fn exists(&self) -> bool {
        self.targets.iter().all(TargetStatus::exists)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationOutcome {
    Updated,
    NotModified,
    Failed,
    DiskUpdatedPendingReload,
}

#[derive(Clone, Debug)]
pub struct OperationSummary {
    pub(crate) source: CatalogSource,
    pub(crate) outcome: OperationOutcome,
    pub(crate) duration: Duration,
    pub(crate) error_code: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) disk_updated: bool,
}

impl OperationSummary {
    pub const fn source(&self) -> CatalogSource {
        self.source
    }
    pub const fn outcome(&self) -> OperationOutcome {
        self.outcome
    }
    pub const fn duration(&self) -> Duration {
        self.duration
    }
    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub const fn disk_updated(&self) -> bool {
        self.disk_updated
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReloadSummary {
    NotNeeded,
    Reloaded,
    Failed { message: String },
}

#[derive(Clone, Debug)]
pub struct RefreshResult {
    pub(crate) ttl_days: f64,
    pub(crate) force: bool,
    pub(crate) check_only: bool,
    pub(crate) requested_sources: Vec<CatalogSource>,
    pub(crate) due_sources: Vec<CatalogSource>,
    pub(crate) refreshed_sources: Vec<CatalogSource>,
    pub(crate) skipped_sources: Vec<CatalogSource>,
    pub(crate) failed_sources: Vec<CatalogSource>,
    pub(crate) statuses: Vec<SourceStatus>,
    pub(crate) operations: Vec<OperationSummary>,
    pub(crate) reload: ReloadSummary,
}

#[derive(Clone, Debug)]
pub struct RefreshPlan {
    pub(crate) statuses: Vec<SourceStatus>,
    pub(crate) due_sources: Vec<CatalogSource>,
    pub(crate) skipped_sources: Vec<CatalogSource>,
}

impl RefreshPlan {
    pub fn statuses(&self) -> &[SourceStatus] {
        &self.statuses
    }
    pub fn due_sources(&self) -> &[CatalogSource] {
        &self.due_sources
    }
    pub fn skipped_sources(&self) -> &[CatalogSource] {
        &self.skipped_sources
    }
}

impl RefreshResult {
    pub const fn ttl_days(&self) -> f64 {
        self.ttl_days
    }
    pub const fn force(&self) -> bool {
        self.force
    }
    pub const fn check_only(&self) -> bool {
        self.check_only
    }
    pub fn requested_sources(&self) -> &[CatalogSource] {
        &self.requested_sources
    }
    pub fn due_sources(&self) -> &[CatalogSource] {
        &self.due_sources
    }
    pub fn refreshed_sources(&self) -> &[CatalogSource] {
        &self.refreshed_sources
    }
    pub fn skipped_sources(&self) -> &[CatalogSource] {
        &self.skipped_sources
    }
    pub fn failed_sources(&self) -> &[CatalogSource] {
        &self.failed_sources
    }
    pub fn statuses(&self) -> &[SourceStatus] {
        &self.statuses
    }
    pub fn operations(&self) -> &[OperationSummary] {
        &self.operations
    }
    pub const fn reload(&self) -> &ReloadSummary {
        &self.reload
    }
}

pub fn source_label(source: CatalogSource) -> &'static str {
    match source {
        CatalogSource::Lxns => "LXNS 曲库和别名",
        CatalogSource::DxData => "dxrating 日服补充曲库",
        CatalogSource::DivingFish => "DivingFish 国服曲库",
        CatalogSource::Yuzu => "Yuzu 别名补充",
        CatalogSource::ChartStats => "Diving-Fish 拟合定数",
        CatalogSource::DxRatingAliases => "dxrating 社区别名",
        CatalogSource::DxRatingTags => "dxrating 社区标签",
        CatalogSource::Plate => "CN 牌子曲目白名单",
        CatalogSource::Location => "华立 maimai 机厅位置",
    }
}
