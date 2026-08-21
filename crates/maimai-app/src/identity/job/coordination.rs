use std::sync::Arc;

use maimai_providers::NapCatClient;
use maimai_storage::{IdentityJob, IdentityJobError, IdentityJobStart, IdentityRefreshReason};
use time::OffsetDateTime;
use tokio::sync::{watch, watch::Sender};

use super::super::{
    IdentityError, IdentityService, RefreshJobLaunch, RefreshJobRequest, RefreshPolicy,
    RefreshReport, ResetHour,
};
use super::{
    persistence::{launch_without_start, recover_if_needed},
    state::{PendingTerminalWrite, RefreshFlight, RefreshOutcome},
};

impl IdentityService {
    pub async fn initialize_identity_jobs(&self, now: OffsetDateTime) -> Result<(), IdentityError> {
        {
            let mut state = self.refresh_state.lock().await;
            recover_if_needed(self.directory.store(), &mut state, now).await?;
        }
        self.reap_background_task(now).await?;
        self.retry_pending_terminal().await
    }

    pub async fn identity_job_status(
        &self,
        now: OffsetDateTime,
    ) -> Result<Option<IdentityJob>, IdentityError> {
        self.initialize_identity_jobs(now).await?;
        Ok(self.directory.store().identity_job().await?)
    }

    pub async fn start_refresh_job(
        &self,
        request: RefreshJobRequest,
        started_at: OffsetDateTime,
        reset_hour: ResetHour,
    ) -> Result<RefreshJobLaunch, IdentityError> {
        self.start_refresh_job_with_owned_client(
            Arc::clone(&self.napcat),
            request,
            started_at,
            reset_hour,
        )
        .await
    }

    pub async fn start_refresh_job_with_client(
        &self,
        client: NapCatClient,
        request: RefreshJobRequest,
        started_at: OffsetDateTime,
        reset_hour: ResetHour,
    ) -> Result<RefreshJobLaunch, IdentityError> {
        self.start_refresh_job_with_owned_client(Arc::new(client), request, started_at, reset_hour)
            .await
    }

    async fn start_refresh_job_with_owned_client(
        &self,
        client: Arc<NapCatClient>,
        request: RefreshJobRequest,
        started_at: OffsetDateTime,
        reset_hour: ResetHour,
    ) -> Result<RefreshJobLaunch, IdentityError> {
        self.initialize_identity_jobs(started_at).await?;
        let mut state = self.refresh_state.lock().await;
        let cache = self.cache_status_unchecked(started_at, reset_hour).await?;
        if state.active.is_some() {
            return launch_without_start(self.directory.store(), cache).await;
        }
        if request.policy != RefreshPolicy::Force && cache.fresh {
            return launch_without_start(self.directory.store(), cache).await;
        }

        let reason = match request.policy {
            RefreshPolicy::Force => IdentityRefreshReason::ForceRefresh,
            RefreshPolicy::IfStale => IdentityRefreshReason::StaleOrMissing,
            RefreshPolicy::AutoDaily => IdentityRefreshReason::AutoDaily,
        };
        let job = match self
            .directory
            .store()
            .start_identity_job(reason, started_at)
            .await?
        {
            IdentityJobStart::AlreadyRunning(job) => {
                return Ok(RefreshJobLaunch {
                    started: false,
                    cache,
                    job: Some(job),
                });
            }
            IdentityJobStart::Started(job) => job,
        };
        let flight = state.start_flight(Some(job.generation))?;
        let flight_id = flight.id;
        let outcome_sender = flight.sender;
        let service = self.clone();
        let generation = job.generation;
        let task = tokio::spawn(async move {
            service
                .run_background_refresh(client, request, generation, outcome_sender)
                .await
        });
        state.attach_task(flight_id, task);
        drop(state);

        Ok(RefreshJobLaunch {
            started: true,
            cache: self.cache_status_unchecked(started_at, reset_hour).await?,
            job: Some(job),
        })
    }

    async fn run_background_refresh(
        &self,
        client: Arc<NapCatClient>,
        request: RefreshJobRequest,
        generation: u64,
        outcome_sender: Sender<Option<RefreshOutcome>>,
    ) -> Result<(), IdentityJobError> {
        let result = self
            .perform_refresh(
                &client,
                request.options,
                OffsetDateTime::now_utc(),
                Some(generation),
            )
            .await;
        let finished_at = OffsetDateTime::now_utc();
        match result {
            Ok(RefreshReport { metadata, .. }) => {
                let terminal = PendingTerminalWrite::Complete {
                    generation,
                    finished_at,
                    stats: metadata.stats,
                };
                if let Err(error) = self.persist_terminal(&terminal).await {
                    self.record_pending_terminal(terminal).await;
                    let safe = error.safe_job_error();
                    let _ = outcome_sender.send(Some(RefreshOutcome::Failed(safe.clone())));
                    return Err(safe);
                }
                let _ = outcome_sender.send(Some(RefreshOutcome::Succeeded(metadata)));
            }
            Err(error) => {
                let safe = error.safe_job_error();
                let terminal = PendingTerminalWrite::Fail {
                    generation,
                    finished_at,
                    error: safe.clone(),
                };
                if let Err(persist_error) = self.persist_terminal(&terminal).await {
                    self.record_pending_terminal(terminal).await;
                    let storage_error = persist_error.safe_job_error();
                    let _ = outcome_sender.send(Some(RefreshOutcome::Failed(safe)));
                    return Err(storage_error);
                }
                let _ = outcome_sender.send(Some(RefreshOutcome::Failed(safe)));
            }
        }
        Ok(())
    }

    pub(crate) async fn begin_refresh_flight(
        &self,
        client: Arc<NapCatClient>,
        request: super::super::RefreshOptions,
        fetched_at: OffsetDateTime,
    ) -> Result<RefreshFlight, IdentityError> {
        let mut state = self.refresh_state.lock().await;
        if let Some(active) = state.active.as_ref() {
            return Ok(RefreshFlight::Follower(active.done.clone()));
        }
        let flight = state.start_flight(None)?;
        let id = flight.id;
        let sender = flight.sender;
        let receiver = flight.receiver;
        let service = self.clone();
        let task = tokio::spawn(async move {
            let result = service
                .perform_refresh(&client, request, fetched_at, None)
                .await;
            let outcome = match result {
                Ok(report) => RefreshOutcome::Succeeded(report.metadata),
                Err(error) => RefreshOutcome::Failed(error.safe_job_error()),
            };
            let _ = sender.send(Some(outcome));
            Ok(())
        });
        state.attach_task(id, task);
        Ok(RefreshFlight::Leader { id, receiver })
    }

    pub(crate) async fn wait_for_refresh(
        &self,
        mut receiver: watch::Receiver<Option<RefreshOutcome>>,
    ) -> Result<RefreshReport, IdentityError> {
        if receiver.borrow().is_none() {
            receiver
                .changed()
                .await
                .map_err(|_| IdentityError::RefreshSignalClosed)?;
        }
        match receiver.borrow().clone() {
            Some(RefreshOutcome::Succeeded(metadata)) => Ok(RefreshReport {
                performed: false,
                metadata,
            }),
            Some(RefreshOutcome::Failed(error)) => Err(IdentityError::RefreshFailed(error)),
            None => Err(IdentityError::RefreshSignalClosed),
        }
    }
}
