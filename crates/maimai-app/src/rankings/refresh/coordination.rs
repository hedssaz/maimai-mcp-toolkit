use std::sync::Arc;

use maimai_core::GroupId;
use maimai_providers::NapCatClient;
use maimai_storage::{RankingJobStart, RankingJobStatus, RankingNamespace, RankingRefreshReason};
use time::OffsetDateTime;

use super::super::{RankingError, RankingLaunch, RankingService, RefreshOptions};

type ActiveRankingTask = (
    (RankingNamespace, String),
    tokio::task::JoinHandle<Result<(), maimai_storage::RankingJobError>>,
);

impl RankingService {
    pub(crate) async fn initialize(&self, now: OffsetDateTime) -> Result<(), RankingError> {
        self.reap_finished(now).await?;
        let store = self.store.clone();
        self.recovery
            .get_or_try_init(|| async move {
                store.interrupt_running_ranking_jobs(now).await?;
                Ok::<(), maimai_storage::StorageError>(())
            })
            .await?;
        self.retry_terminal_errors(now).await?;
        Ok(())
    }

    pub async fn ensure_cache(
        &self,
        namespace: RankingNamespace,
        group_id: GroupId,
        force: bool,
        options: RefreshOptions,
        now: OffsetDateTime,
    ) -> Result<Option<RankingLaunch>, RankingError> {
        self.ensure_cache_with_client(
            namespace,
            group_id,
            force,
            options,
            now,
            Arc::clone(&self.napcat),
        )
        .await
    }

    pub async fn ensure_cache_with_client(
        &self,
        namespace: RankingNamespace,
        group_id: GroupId,
        force: bool,
        options: RefreshOptions,
        now: OffsetDateTime,
        client: Arc<NapCatClient>,
    ) -> Result<Option<RankingLaunch>, RankingError> {
        self.initialize(now).await?;
        let cache = self
            .cache_status_unchecked(namespace, &group_id, now)
            .await?;
        let reason = if force {
            RankingRefreshReason::ForceRefresh
        } else if cache.exists {
            RankingRefreshReason::Stale
        } else {
            RankingRefreshReason::Miss
        };
        if !force && cache.fresh {
            return Ok(None);
        }

        let job = match self
            .store
            .start_ranking_job(namespace, &group_id, reason, now)
            .await?
        {
            RankingJobStart::Started(job) => job,
            RankingJobStart::AlreadyRunning(job) => {
                return Ok(Some(RankingLaunch {
                    started: false,
                    reason: job.refresh_reason,
                    cache,
                    job,
                }));
            }
        };
        let service = self.clone();
        let task_group = group_id.clone();
        let generation = job.generation;
        let task = tokio::spawn(async move {
            service
                .run_background(namespace, task_group, generation, client, options, now)
                .await
        });
        self.state
            .lock()
            .await
            .active
            .insert((namespace, group_id.as_str().to_owned()), task);
        Ok(Some(RankingLaunch {
            started: true,
            reason,
            cache,
            job,
        }))
    }

    async fn reap_finished(&self, now: OffsetDateTime) -> Result<(), RankingError> {
        let finished = {
            let mut state = self.state.lock().await;
            let keys = state
                .active
                .iter()
                .filter_map(|(key, task)| task.is_finished().then_some(key.clone()))
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| state.active.remove(&key).map(|task| (key, task)))
                .collect::<Vec<_>>()
        };
        self.settle_tasks(finished, now).await
    }

    async fn settle_tasks(
        &self,
        tasks: Vec<ActiveRankingTask>,
        now: OffsetDateTime,
    ) -> Result<(), RankingError> {
        let mut first_error = None;
        for ((namespace, group_id), task) in tasks {
            if let Err(error) = self.settle_task(namespace, group_id, task, now).await {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn settle_task(
        &self,
        namespace: RankingNamespace,
        group_id: String,
        task: tokio::task::JoinHandle<Result<(), maimai_storage::RankingJobError>>,
        now: OffsetDateTime,
    ) -> Result<(), RankingError> {
        match task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                self.state
                    .lock()
                    .await
                    .terminal_errors
                    .insert((namespace, group_id), error);
                Ok(())
            }
            Err(_) => self.persist_join_failure(namespace, group_id, now).await,
        }
    }

    async fn persist_join_failure(
        &self,
        namespace: RankingNamespace,
        group_id: String,
        now: OffsetDateTime,
    ) -> Result<(), RankingError> {
        let error = RankingError::Task.safe_job_error();
        let group = match GroupId::new(&group_id) {
            Ok(group) => group,
            Err(_) => {
                self.state
                    .lock()
                    .await
                    .terminal_errors
                    .insert((namespace, group_id), error);
                return Err(RankingError::InvalidInput { field: "groupId" });
            }
        };
        let job = match self.store.ranking_job(namespace, &group).await {
            Ok(job) => job,
            Err(storage) => {
                self.state
                    .lock()
                    .await
                    .terminal_errors
                    .insert((namespace, group_id), error);
                return Err(storage.into());
            }
        };
        let Some(job) = job.filter(|job| job.status == RankingJobStatus::Running) else {
            return Ok(());
        };
        match self
            .store
            .fail_ranking_job(namespace, &group, job.generation, now, &error)
            .await
        {
            Ok(true) => Ok(()),
            Ok(false) => {
                self.state
                    .lock()
                    .await
                    .terminal_errors
                    .insert((namespace, group_id), error);
                Ok(())
            }
            Err(storage) => {
                self.state
                    .lock()
                    .await
                    .terminal_errors
                    .insert((namespace, group_id), error);
                Err(storage.into())
            }
        }
    }

    /// Drains every background ranking refresh and retries persisted terminal
    /// failures before the shared state store is closed.
    pub async fn shutdown(&self, now: OffsetDateTime) -> Result<(), RankingError> {
        let active = self.state.lock().await.active.drain().collect::<Vec<_>>();
        let mut first_error = self.settle_tasks(active, now).await.err();
        if let Err(error) = self.retry_terminal_errors(now).await {
            first_error.get_or_insert(error);
        }
        if !self.state.lock().await.active.is_empty() {
            first_error.get_or_insert(RankingError::Task);
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn retry_terminal_errors(&self, now: OffsetDateTime) -> Result<(), RankingError> {
        let pending = self
            .state
            .lock()
            .await
            .terminal_errors
            .iter()
            .map(|(key, error)| (key.clone(), error.clone()))
            .collect::<Vec<_>>();
        let mut first_error = None;
        for (key, error) in pending {
            if let Err(failure) = self.retry_terminal_error(key, error, now).await {
                first_error.get_or_insert(failure);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn retry_terminal_error(
        &self,
        key: (RankingNamespace, String),
        error: maimai_storage::RankingJobError,
        now: OffsetDateTime,
    ) -> Result<(), RankingError> {
        let (namespace, group_id) = &key;
        let group =
            GroupId::new(group_id).map_err(|_| RankingError::InvalidInput { field: "groupId" })?;
        let Some(job) = self.store.ranking_job(*namespace, &group).await? else {
            self.state.lock().await.terminal_errors.remove(&key);
            return Ok(());
        };
        if job.status != RankingJobStatus::Running
            || self
                .store
                .fail_ranking_job(*namespace, &group, job.generation, now, &error)
                .await?
        {
            self.state.lock().await.terminal_errors.remove(&key);
        }
        Ok(())
    }
}
