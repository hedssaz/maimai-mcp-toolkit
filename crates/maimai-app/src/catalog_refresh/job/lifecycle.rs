use maimai_storage::{
    CatalogRefreshJobId, CatalogRefreshJobStore, CatalogRefreshOwner, CatalogRefreshTerminalUpdate,
};
use time::OffsetDateTime;
use tokio::task::JoinHandle;

use super::{CatalogRefreshJobs, RefreshJobError};

#[derive(Default)]
pub(super) struct JobRuntime {
    pub(super) active: Option<ActiveTask>,
    pub(super) shutting_down: bool,
    pending_terminal: Option<PendingTerminal>,
}

pub(super) struct ActiveTask {
    pub(super) id: CatalogRefreshJobId,
    pub(super) task: JoinHandle<()>,
}

#[derive(Clone)]
pub(super) enum PendingTerminal {
    Finish {
        id: CatalogRefreshJobId,
        update: CatalogRefreshTerminalUpdate,
    },
    FailActive {
        id: CatalogRefreshJobId,
        code: String,
        message: String,
    },
}

impl CatalogRefreshJobs {
    pub(super) async fn maintain(&self) -> Result<(), RefreshJobError> {
        let finished = {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| RefreshJobError::RuntimePoisoned)?;
            if runtime
                .active
                .as_ref()
                .is_some_and(|active| active.task.is_finished())
            {
                runtime.active.take()
            } else {
                None
            }
        };
        if let Some(active) = finished
            && active.task.await.is_err()
        {
            let pending = PendingTerminal::FailActive {
                id: active.id,
                code: "TASK_ABORTED".to_owned(),
                message: "refresh job task terminated before completion".to_owned(),
            };
            if self.persist_pending(&pending).await.is_err() {
                self.record_pending(pending);
            }
        }
        let pending = self
            .runtime
            .lock()
            .map_err(|_| RefreshJobError::RuntimePoisoned)?
            .pending_terminal
            .take();
        if let Some(pending) = pending
            && let Err(error) = self.persist_pending(&pending).await
        {
            self.record_pending(pending);
            return Err(error);
        }
        Ok(())
    }

    async fn persist_pending(&self, pending: &PendingTerminal) -> Result<(), RefreshJobError> {
        let changed = match pending {
            PendingTerminal::Finish { id, update } => {
                self.store
                    .finish_catalog_refresh_job(*id, OffsetDateTime::now_utc(), update)
                    .await?
            }
            PendingTerminal::FailActive { id, code, message } => {
                self.store
                    .fail_active_catalog_refresh_job(*id, OffsetDateTime::now_utc(), code, message)
                    .await?
            }
        };
        if changed {
            Ok(())
        } else {
            Err(RefreshJobError::TransitionRejected)
        }
    }

    pub(super) fn record_pending(&self, pending: PendingTerminal) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.pending_terminal = Some(pending);
        }
    }

    /// Stops this process' active refresh, records a terminal state, releases
    /// the database owner claim, and closes the narrow SQLite pool.
    pub async fn shutdown(&self) -> Result<(), RefreshJobError> {
        let _lifecycle = self.lifecycle.lock().await;
        let active = {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| RefreshJobError::RuntimePoisoned)?;
            if runtime.shutting_down {
                return Ok(());
            }
            runtime.shutting_down = true;
            runtime.active.take()
        };

        if let Some(active) = active {
            if !active.task.is_finished() {
                active.task.abort();
            }
            let _task_result = active.task.await;
        }

        let mut first_error = self.persist_recorded_pending().await.err();
        match self.store.active_catalog_refresh_job().await {
            Ok(Some(job)) => {
                if let Err(error) = self
                    .store
                    .interrupt_catalog_refresh_job(job.id, OffsetDateTime::now_utc())
                    .await
                {
                    first_error.get_or_insert_with(|| error.into());
                }
            }
            Ok(None) => {}
            Err(error) => {
                first_error.get_or_insert_with(|| error.into());
            }
        }
        match self.store.release_owner(&self.owner) {
            Ok(true) => {}
            Ok(false) => {
                first_error.get_or_insert(RefreshJobError::TransitionRejected);
            }
            Err(error) => {
                first_error.get_or_insert_with(|| error.into());
            }
        }
        self.store.close().await;
        first_error.map_or(Ok(()), Err)
    }

    async fn persist_recorded_pending(&self) -> Result<(), RefreshJobError> {
        let pending = self
            .runtime
            .lock()
            .map_err(|_| RefreshJobError::RuntimePoisoned)?
            .pending_terminal
            .take();
        if let Some(pending) = pending {
            self.persist_pending(&pending).await
        } else {
            Ok(())
        }
    }
}

pub(super) fn release_startup_owner(
    store: &CatalogRefreshJobStore,
    owner: &CatalogRefreshOwner,
) -> Result<(), RefreshJobError> {
    if store.release_owner(owner)? {
        Ok(())
    } else {
        Err(RefreshJobError::TransitionRejected)
    }
}
