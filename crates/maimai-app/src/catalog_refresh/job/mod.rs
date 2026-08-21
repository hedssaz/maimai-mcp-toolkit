mod lifecycle;

use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use maimai_storage::{
    CatalogRefreshJobStart, CatalogRefreshJobStore, CatalogRefreshOwner, CatalogRefreshOwnerClaim,
    CatalogRefreshSourceUpdate, CatalogRefreshTerminalUpdate, StorageError,
};
use thiserror::Error;
use time::OffsetDateTime;
use tokio::sync::{Mutex as AsyncMutex, mpsc};

use crate::catalog_refresh::{
    CatalogRefreshService, OperationOutcome, OperationSummary, RefreshError, RefreshPlan,
    RefreshProgress, RefreshRequest, RefreshResult, ReloadSummary,
};

use lifecycle::{ActiveTask, JobRuntime, PendingTerminal, release_startup_owner};
pub use maimai_storage::{
    CatalogRefreshJob, CatalogRefreshJobId, CatalogRefreshJobOutcome, CatalogRefreshJobSource,
    CatalogRefreshJobStatus, CatalogRefreshSourceStatus,
};

#[derive(Debug, Error)]
pub enum RefreshJobError {
    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error("refresh job transition was rejected")]
    TransitionRejected,

    #[error(transparent)]
    Refresh(#[from] RefreshError),

    #[error("refresh job coordinator state is poisoned")]
    RuntimePoisoned,

    #[error(
        "catalog refresh state is owned by another process (lock={lock_path}, job={active_job_id:?})"
    )]
    AlreadyRunning {
        lock_path: std::path::PathBuf,
        active_job_id: Option<CatalogRefreshJobId>,
    },

    #[error("catalog refresh job coordinator is shutting down")]
    ShuttingDown,
}

#[derive(Clone, Debug)]
pub struct StartRefreshJob {
    record: CatalogRefreshJob,
    started: bool,
    plan: Option<RefreshPlan>,
}

impl StartRefreshJob {
    pub const fn record(&self) -> &CatalogRefreshJob {
        &self.record
    }
    pub const fn started(&self) -> bool {
        self.started
    }
    pub const fn plan(&self) -> Option<&RefreshPlan> {
        self.plan.as_ref()
    }
}

pub struct CatalogRefreshJobs {
    service: Arc<CatalogRefreshService>,
    store: CatalogRefreshJobStore,
    owner: CatalogRefreshOwner,
    lifecycle: AsyncMutex<()>,
    runtime: Mutex<JobRuntime>,
}

/// Coordinates the single catalog-refresh owner for one state database.
///
/// One database supports one live catalog process. An OS advisory lock rejects a
/// second live owner. The kernel releases the lock after a crash, so the next owner
/// can mark the abandoned active job interrupted without PID, heartbeat, or lease
/// heuristics. Normal callers must invoke [`Self::shutdown`] after stdio ends.
impl CatalogRefreshJobs {
    pub async fn open(
        service: Arc<CatalogRefreshService>,
        store: CatalogRefreshJobStore,
    ) -> Result<Self, RefreshJobError> {
        let owner = match store.claim_owner()? {
            CatalogRefreshOwnerClaim::Acquired(owner) => owner,
            CatalogRefreshOwnerClaim::AlreadyOwned { lock_path } => {
                let active_job_id = store
                    .active_catalog_refresh_job()
                    .await
                    .ok()
                    .flatten()
                    .map(|job| job.id);
                return Err(RefreshJobError::AlreadyRunning {
                    lock_path,
                    active_job_id,
                });
            }
        };
        if let Err(error) = store
            .interrupt_catalog_refresh_jobs(OffsetDateTime::now_utc())
            .await
        {
            release_startup_owner(&store, &owner)?;
            return Err(error.into());
        }
        Ok(Self {
            service,
            store,
            owner,
            lifecycle: AsyncMutex::new(()),
            runtime: Mutex::new(JobRuntime::default()),
        })
    }

    pub async fn start(
        self: &Arc<Self>,
        request: RefreshRequest,
    ) -> Result<StartRefreshJob, RefreshJobError> {
        self.maintain().await?;
        let service = Arc::clone(&self.service);
        self.start_with(request, move |request, sender| async move {
            service
                .refresh_reporting(request, move |progress| {
                    let _ = sender.send(progress);
                })
                .await
        })
        .await
    }

    async fn start_with<F, Fut>(
        self: &Arc<Self>,
        request: RefreshRequest,
        run: F,
    ) -> Result<StartRefreshJob, RefreshJobError>
    where
        F: FnOnce(RefreshRequest, mpsc::UnboundedSender<RefreshProgress>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<RefreshResult, RefreshError>> + Send + 'static,
    {
        let sources = request
            .sources()
            .iter()
            .map(|source| source.name().to_owned())
            .collect::<Vec<_>>();
        let plan = self.service.plan(&request).await?;
        let due = names(plan.due_sources());
        let _lifecycle = self.lifecycle.lock().await;
        if self
            .runtime
            .lock()
            .map_err(|_| RefreshJobError::RuntimePoisoned)?
            .shutting_down
        {
            return Err(RefreshJobError::ShuttingDown);
        }
        match self
            .store
            .start_catalog_refresh_job(&sources, &due, OffsetDateTime::now_utc())
            .await?
        {
            CatalogRefreshJobStart::AlreadyRunning(record) => Ok(StartRefreshJob {
                record,
                started: false,
                plan: None,
            }),
            CatalogRefreshJobStart::Started(record) => {
                let job_id = record.id;
                let jobs = Arc::clone(self);
                let task = tokio::spawn(async move {
                    jobs.run_job(job_id, request, run).await;
                });
                self.runtime
                    .lock()
                    .map_err(|_| RefreshJobError::RuntimePoisoned)?
                    .active = Some(ActiveTask { id: job_id, task });
                Ok(StartRefreshJob {
                    record,
                    started: true,
                    plan: Some(plan),
                })
            }
        }
    }

    pub async fn status(
        &self,
        id: CatalogRefreshJobId,
    ) -> Result<Option<CatalogRefreshJob>, RefreshJobError> {
        self.maintain().await?;
        Ok(self.store.catalog_refresh_job(id).await?)
    }

    async fn run_job<F, Fut>(
        self: Arc<Self>,
        id: CatalogRefreshJobId,
        request: RefreshRequest,
        run: F,
    ) where
        F: FnOnce(RefreshRequest, mpsc::UnboundedSender<RefreshProgress>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<RefreshResult, RefreshError>> + Send + 'static,
    {
        let running = self
            .store
            .mark_catalog_refresh_job_running(id, OffsetDateTime::now_utc())
            .await;
        match running {
            Ok(true) => {}
            Ok(false) => return,
            Err(_) => {
                self.record_pending(PendingTerminal::FailActive {
                    id,
                    code: "STATE_WRITE_FAILED".to_owned(),
                    message: "running job state could not be persisted".to_owned(),
                });
                return;
            }
        }
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let progress_jobs = Arc::clone(&self);
        let progress = async move {
            let mut failed = false;
            while let Some(progress) = receiver.recv().await {
                if progress_jobs.persist_progress(id, progress).await.is_err() {
                    failed = true;
                }
            }
            failed
        };
        let execution = async move { run(request, sender).await };
        let (result, progress_failed) = tokio::join!(execution, progress);
        let terminal = match result {
            Ok(result) if !progress_failed => {
                if self.sync_final_sources(id, &result).await.is_err() {
                    storage_failed_terminal()
                } else {
                    terminal_from_result(&result)
                }
            }
            Ok(_) => storage_failed_terminal(),
            Err(error) => CatalogRefreshTerminalUpdate {
                status: CatalogRefreshJobStatus::Failed,
                outcome: CatalogRefreshJobOutcome::Failed,
                message: "刷新任务失败。".to_owned(),
                error_code: Some("REFRESH_FAILED".to_owned()),
                error_message: Some(error.to_string()),
            },
        };
        let first = self
            .store
            .finish_catalog_refresh_job(id, OffsetDateTime::now_utc(), &terminal)
            .await;
        if !matches!(first, Ok(true)) {
            self.record_pending(PendingTerminal::Finish {
                id,
                update: terminal,
            });
        }
    }

    async fn persist_progress(
        &self,
        id: CatalogRefreshJobId,
        progress: RefreshProgress,
    ) -> Result<(), StorageError> {
        match progress {
            RefreshProgress::Planned { due, skipped } => {
                let due = names(&due);
                let skipped = names(&skipped);
                require_transition(
                    self.store
                        .plan_catalog_refresh_job(id, &due, &skipped)
                        .await?,
                )
            }
            RefreshProgress::SourceFinished(operation) => require_transition(
                self.store
                    .update_catalog_refresh_source(id, &source_update(&operation))
                    .await?,
            ),
        }
    }

    async fn sync_final_sources(
        &self,
        id: CatalogRefreshJobId,
        result: &RefreshResult,
    ) -> Result<(), StorageError> {
        for source in result.requested_sources() {
            let update = result
                .operations()
                .iter()
                .find(|operation| operation.source() == *source)
                .map(source_update)
                .unwrap_or_else(|| CatalogRefreshSourceUpdate {
                    source: source.name().to_owned(),
                    status: CatalogRefreshSourceStatus::Skipped,
                    duration_millis: 0,
                    error_code: None,
                    error_message: None,
                });
            require_transition(
                self.store
                    .update_catalog_refresh_source(id, &update)
                    .await?,
            )?;
        }
        Ok(())
    }
}

fn source_update(operation: &OperationSummary) -> CatalogRefreshSourceUpdate {
    CatalogRefreshSourceUpdate {
        source: operation.source().name().to_owned(),
        status: match operation.outcome() {
            OperationOutcome::Updated => CatalogRefreshSourceStatus::Updated,
            OperationOutcome::NotModified => CatalogRefreshSourceStatus::NotModified,
            OperationOutcome::Failed => CatalogRefreshSourceStatus::Failed,
            OperationOutcome::DiskUpdatedPendingReload => {
                CatalogRefreshSourceStatus::DiskUpdatedPendingReload
            }
        },
        duration_millis: u64::try_from(operation.duration().as_millis()).unwrap_or(u64::MAX),
        error_code: operation.error_code().map(str::to_owned),
        error_message: operation.error().map(str::to_owned),
    }
}

fn terminal_from_result(result: &RefreshResult) -> CatalogRefreshTerminalUpdate {
    let failed = !result.failed_sources().is_empty()
        || matches!(result.reload(), ReloadSummary::Failed { .. });
    let outcome = if failed && result.refreshed_sources().is_empty() {
        CatalogRefreshJobOutcome::Failed
    } else if failed {
        CatalogRefreshJobOutcome::PartialFailure
    } else {
        CatalogRefreshJobOutcome::Success
    };
    CatalogRefreshTerminalUpdate {
        status: CatalogRefreshJobStatus::Completed,
        outcome,
        message: if failed {
            format!(
                "刷新完成: 成功 {}/{}，失败 {}",
                result.refreshed_sources().len() + result.skipped_sources().len(),
                result.requested_sources().len(),
                result.failed_sources().len()
            )
        } else {
            format!(
                "刷新完成: 成功 {}/{}",
                result.refreshed_sources().len() + result.skipped_sources().len(),
                result.requested_sources().len()
            )
        },
        error_code: failed.then(|| "SOURCE_FAILURE".to_owned()),
        error_message: failed.then(|| "one or more catalog sources were not published".to_owned()),
    }
}

fn storage_failed_terminal() -> CatalogRefreshTerminalUpdate {
    CatalogRefreshTerminalUpdate {
        status: CatalogRefreshJobStatus::Failed,
        outcome: CatalogRefreshJobOutcome::Failed,
        message: "刷新任务状态写入失败。".to_owned(),
        error_code: Some("STATE_WRITE_FAILED".to_owned()),
        error_message: Some("source progress could not be persisted".to_owned()),
    }
}

fn require_transition(changed: bool) -> Result<(), StorageError> {
    if changed {
        Ok(())
    } else {
        Err(StorageError::InvalidStoredValue {
            field: "catalog_refresh_job.transition",
            value: "CAS rejected".to_owned(),
        })
    }
}

fn names(sources: &[maimai_providers::CatalogSource]) -> Vec<String> {
    sources
        .iter()
        .map(|source| source.name().to_owned())
        .collect()
}

#[cfg(test)]
mod tests;
