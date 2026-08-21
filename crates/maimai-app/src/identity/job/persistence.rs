use maimai_storage::{IdentityJobError, IdentityJobErrorCode, StateStore};
use time::OffsetDateTime;
use tokio::task::JoinHandle;

use super::super::{IdentityError, IdentityService, RefreshJobLaunch};
use super::state::{PendingTerminalWrite, RefreshState};

impl IdentityService {
    /// Waits for the current refresh to settle, persists any missing terminal
    /// state, and leaves the coordinator without an active task.
    pub async fn shutdown(&self, now: OffsetDateTime) -> Result<(), IdentityError> {
        let active = self.refresh_state.lock().await.active.take();
        let mut first_error = None;
        if let Some(mut active) = active {
            match active.task.take() {
                Some(task) => match task.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        if self.refresh_state.lock().await.pending_terminal.is_none() {
                            first_error = Some(IdentityError::RefreshFailed(error));
                        }
                    }
                    Err(_) => {
                        if let Some(generation) = active.generation {
                            let terminal = join_failure(generation, now);
                            if self.persist_terminal(&terminal).await.is_err() {
                                self.record_pending_terminal(terminal).await;
                            }
                        }
                        first_error = Some(IdentityError::RefreshTaskJoin);
                    }
                },
                None => {
                    if let Some(generation) = active.generation {
                        let terminal = join_failure(generation, now);
                        if self.persist_terminal(&terminal).await.is_err() {
                            self.record_pending_terminal(terminal).await;
                        }
                    }
                    first_error = Some(IdentityError::RefreshTaskJoin);
                }
            }
        }
        if let Err(error) = self.retry_pending_terminal().await {
            first_error.get_or_insert(error);
        }
        if self.refresh_state.lock().await.active.is_some() {
            first_error.get_or_insert(IdentityError::RefreshTaskJoin);
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(crate) async fn reap_direct_refresh(&self, flight_id: u64) -> Result<(), IdentityError> {
        let active = {
            let mut state = self.refresh_state.lock().await;
            let is_direct_flight = state
                .active
                .as_ref()
                .is_some_and(|active| active.id == flight_id && active.generation.is_none());
            is_direct_flight.then(|| state.active.take()).flatten()
        };
        let Some(mut active) = active else {
            return Ok(());
        };
        let Some(task) = active.task.take() else {
            return Err(IdentityError::RefreshTaskJoin);
        };
        match task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(IdentityError::RefreshFailed(error)),
            Err(_) => Err(IdentityError::RefreshTaskJoin),
        }
    }

    pub(super) async fn persist_terminal(
        &self,
        terminal: &PendingTerminalWrite,
    ) -> Result<(), IdentityError> {
        let persisted = match terminal {
            PendingTerminalWrite::Complete {
                generation,
                finished_at,
                stats,
            } => {
                self.directory
                    .store()
                    .complete_identity_job(*generation, *finished_at, *stats)
                    .await?
            }
            PendingTerminalWrite::Fail {
                generation,
                finished_at,
                error,
            } => {
                self.directory
                    .store()
                    .fail_identity_job(*generation, *finished_at, error)
                    .await?
            }
        };
        if persisted {
            Ok(())
        } else {
            Err(IdentityError::TerminalPersistence)
        }
    }

    pub(super) async fn record_pending_terminal(&self, terminal: PendingTerminalWrite) {
        let mut state = self.refresh_state.lock().await;
        state.pending_terminal = Some(terminal);
    }

    pub(super) async fn retry_pending_terminal(&self) -> Result<(), IdentityError> {
        let terminal = {
            let mut state = self.refresh_state.lock().await;
            state.pending_terminal.take()
        };
        let Some(terminal) = terminal else {
            return Ok(());
        };
        match self.persist_terminal(&terminal).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let mut state = self.refresh_state.lock().await;
                if state.pending_terminal.is_none() {
                    state.pending_terminal = Some(terminal);
                }
                Err(error)
            }
        }
    }

    pub(super) async fn reap_background_task(
        &self,
        now: OffsetDateTime,
    ) -> Result<(), IdentityError> {
        let active = {
            let mut state = self.refresh_state.lock().await;
            let finished = state
                .active
                .as_ref()
                .and_then(|active| active.task.as_ref())
                .is_some_and(JoinHandle::is_finished);
            finished.then(|| state.active.take()).flatten()
        };
        let Some(mut active) = active else {
            return Ok(());
        };
        let Some(task) = active.task.take() else {
            return Ok(());
        };
        match task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                if self.refresh_state.lock().await.pending_terminal.is_some() {
                    Ok(())
                } else {
                    Err(IdentityError::RefreshFailed(error))
                }
            }
            Err(_) => {
                if let Some(generation) = active.generation {
                    let terminal = join_failure(generation, now);
                    if self.persist_terminal(&terminal).await.is_err() {
                        self.record_pending_terminal(terminal).await;
                    }
                }
                Err(IdentityError::RefreshTaskJoin)
            }
        }
    }
}

fn join_failure(generation: u64, finished_at: OffsetDateTime) -> PendingTerminalWrite {
    PendingTerminalWrite::Fail {
        generation,
        finished_at,
        error: IdentityJobError {
            code: IdentityJobErrorCode::Unknown,
            message: "QQ 身份缓存刷新任务异常终止。".to_owned(),
            status: None,
            body: None,
        },
    }
}

pub(super) async fn recover_if_needed(
    store: &StateStore,
    state: &mut RefreshState,
    now: OffsetDateTime,
) -> Result<(), IdentityError> {
    if !state.recovered {
        store.interrupt_running_identity_job(now).await?;
        state.recovered = true;
    }
    Ok(())
}

pub(super) async fn launch_without_start(
    store: &StateStore,
    cache: super::super::CacheStatus,
) -> Result<RefreshJobLaunch, IdentityError> {
    Ok(RefreshJobLaunch {
        started: false,
        cache,
        job: store.identity_job().await?,
    })
}

#[cfg(test)]
mod tests {
    use std::{error::Error, time::Duration};

    use maimai_providers::{NapCatClient, NapCatConfig};
    use maimai_storage::{IdentityJobError, StateStore};
    use tempfile::TempDir;
    use time::OffsetDateTime;
    use tokio::sync::oneshot;
    use url::Url;

    use super::super::super::IdentityService;

    type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

    #[tokio::test]
    async fn shutdown_waits_for_active_refresh_and_clears_state() -> TestResult {
        let temp = TempDir::new()?;
        let store = StateStore::open(temp.path().join("state.db")).await?;
        let napcat = NapCatClient::new(NapCatConfig::new(
            Url::parse("http://127.0.0.1:9/")?,
            Duration::from_secs(1),
            None,
        )?)?;
        let service = IdentityService::new(store.clone(), napcat);
        let (release, blocked) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _released = blocked.await;
            Ok::<(), IdentityJobError>(())
        });
        {
            let mut state = service.refresh_state.lock().await;
            let flight = state.start_flight(None)?;
            state.attach_task(flight.id, task);
        }

        let shutdown_service = service.clone();
        let shutdown =
            tokio::spawn(async move { shutdown_service.shutdown(OffsetDateTime::now_utc()).await });
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        release
            .send(())
            .map_err(|_| std::io::Error::other("release receiver closed"))?;
        shutdown.await??;
        assert!(service.refresh_state.lock().await.active.is_none());
        store.close().await;
        Ok(())
    }
}
