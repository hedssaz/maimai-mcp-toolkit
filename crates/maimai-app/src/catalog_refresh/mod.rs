mod error;
mod fetch;
pub mod job;
mod model;
mod orchestrate;
mod publish;
mod targets;

use std::{future::Future, sync::Arc, time::SystemTime};

use maimai_catalog::CatalogStore;
use maimai_providers::{CatalogSource, CatalogSourceClient, EntityTag};
use tokio::{sync::Mutex, task};

pub use error::{RefreshError, RefreshErrorCode, timeout_duration};
pub use model::{
    DEFAULT_TIMEOUT_SECONDS, DEFAULT_TTL_DAYS, EnabledSources, OperationOutcome, OperationSummary,
    RefreshPlan, RefreshRequest, RefreshResult, ReloadSummary, SourceStatus, TargetStatus,
    source_label,
};

use fetch::{FetchFailure, FetchedBundle};
use model::ReloadSummary as Reload;
use publish::PublishOutcome;
use targets::TargetPaths;

#[derive(Clone, Debug)]
pub(crate) enum RefreshProgress {
    Planned {
        due: Vec<CatalogSource>,
        skipped: Vec<CatalogSource>,
    },
    SourceFinished(OperationSummary),
}

pub struct CatalogRefreshService {
    client: Arc<CatalogSourceClient>,
    catalog: Arc<CatalogStore>,
    enabled: EnabledSources,
    targets: TargetPaths,
    refresh_lock: Mutex<()>,
}

impl CatalogRefreshService {
    pub fn new(
        client: Arc<CatalogSourceClient>,
        catalog: Arc<CatalogStore>,
        enabled: EnabledSources,
    ) -> Result<Self, RefreshError> {
        let targets = TargetPaths::new(catalog.files(), &enabled)?;
        Ok(Self {
            client,
            catalog,
            enabled,
            targets,
            refresh_lock: Mutex::new(()),
        })
    }

    pub const fn enabled_sources(&self) -> &EnabledSources {
        &self.enabled
    }

    pub async fn plan(&self, request: &RefreshRequest) -> Result<RefreshPlan, RefreshError> {
        self.validate_sources(request.sources())?;
        self.plan_at(request, SystemTime::now()).await
    }

    pub async fn refresh(&self, request: RefreshRequest) -> Result<RefreshResult, RefreshError> {
        self.refresh_reporting(request, |_| {}).await
    }

    pub(crate) async fn refresh_reporting<P>(
        &self,
        request: RefreshRequest,
        progress: P,
    ) -> Result<RefreshResult, RefreshError>
    where
        P: Fn(RefreshProgress) + Clone + Send + Sync + 'static,
    {
        self.validate_sources(request.sources())?;
        let _guard = self.refresh_lock.lock().await;
        let client = Arc::clone(&self.client);
        self.refresh_with_progress(
            request,
            SystemTime::now(),
            move |source, etag| {
                let client = Arc::clone(&client);
                async move {
                    client
                        .fetch_with_etag(source, etag.as_ref())
                        .await
                        .map(FetchedBundle::from)
                        .map_err(FetchFailure::from)
                }
            },
            progress,
        )
        .await
    }

    #[cfg(test)]
    async fn refresh_with<F, Fut>(
        &self,
        request: RefreshRequest,
        now: SystemTime,
        fetch: F,
    ) -> Result<RefreshResult, RefreshError>
    where
        F: Fn(CatalogSource, Option<EntityTag>) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Result<FetchedBundle, FetchFailure>> + Send + 'static,
    {
        self.refresh_with_progress(request, now, fetch, |_| {})
            .await
    }

    async fn refresh_with_progress<F, Fut, P>(
        &self,
        request: RefreshRequest,
        now: SystemTime,
        fetch: F,
        progress: P,
    ) -> Result<RefreshResult, RefreshError>
    where
        F: Fn(CatalogSource, Option<EntityTag>) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Result<FetchedBundle, FetchFailure>> + Send + 'static,
        P: Fn(RefreshProgress) + Clone + Send + Sync + 'static,
    {
        let plan = self.plan_at(&request, now).await?;
        let statuses_before = plan.statuses;
        let due_sources = plan.due_sources;
        let skipped_sources = plan.skipped_sources;
        progress(RefreshProgress::Planned {
            due: due_sources.clone(),
            skipped: skipped_sources.clone(),
        });
        if request.check_only() || due_sources.is_empty() {
            return Ok(RefreshResult {
                ttl_days: request.ttl_days(),
                force: request.force(),
                check_only: request.check_only(),
                requested_sources: request.sources().to_vec(),
                due_sources,
                refreshed_sources: Vec::new(),
                skipped_sources,
                failed_sources: Vec::new(),
                statuses: statuses_before,
                operations: Vec::new(),
                reload: Reload::NotNeeded,
            });
        }

        let etag = if due_sources.contains(&CatalogSource::DivingFish) {
            let paths = self.targets.clone();
            task::spawn_blocking(move || targets::read_etag(&paths))
                .await
                .map_err(RefreshError::Join)??
        } else {
            None
        };
        let fetched = self
            .pipe_fetch_due(&due_sources, request.timeout(), etag, fetch)
            .await?;
        let mut operations = Vec::with_capacity(due_sources.len());
        let mut refreshed_sources = Vec::new();
        let mut failed_sources = Vec::new();
        let mut disk_changed = false;
        let mut unsafe_disk_state = false;

        for (index, source) in due_sources.iter().copied().enumerate() {
            let fetched = fetched
                .get(&index)
                .ok_or_else(|| RefreshError::InvalidTarget {
                    catalog_source: source.name(),
                    target: "fetch result",
                })?;
            match &fetched.result {
                Err(failure) => {
                    failed_sources.push(source);
                    let operation = orchestrate::failed_operation(source, fetched.elapsed, failure);
                    progress(RefreshProgress::SourceFinished(operation.clone()));
                    operations.push(operation);
                }
                Ok(bundle) if bundle.source != source => {
                    failed_sources.push(source);
                    let operation = OperationSummary {
                        source,
                        outcome: OperationOutcome::Failed,
                        duration: fetched.elapsed,
                        error_code: Some("INVALID_SOURCE_BUNDLE".to_owned()),
                        error: Some("provider returned a bundle for another source".to_owned()),
                        disk_updated: false,
                    };
                    progress(RefreshProgress::SourceFinished(operation.clone()));
                    operations.push(operation);
                }
                Ok(bundle) => {
                    let paths = self.targets.clone();
                    let bundle = bundle.clone();
                    let started = std::time::Instant::now();
                    let published =
                        task::spawn_blocking(move || publish::publish(&paths, bundle, now))
                            .await
                            .map_err(RefreshError::Join)?;
                    let duration = fetched.elapsed.saturating_add(started.elapsed());
                    match published {
                        Ok(outcome) => {
                            disk_changed = true;
                            refreshed_sources.push(source);
                            let operation = OperationSummary {
                                source,
                                outcome: match outcome {
                                    PublishOutcome::Updated => OperationOutcome::Updated,
                                    PublishOutcome::NotModified => OperationOutcome::NotModified,
                                },
                                duration,
                                error_code: None,
                                error: None,
                                disk_updated: true,
                            };
                            progress(RefreshProgress::SourceFinished(operation.clone()));
                            operations.push(operation);
                        }
                        Err(failure) => {
                            unsafe_disk_state |= failure.disk_updated;
                            failed_sources.push(source);
                            let operation = OperationSummary {
                                source,
                                outcome: OperationOutcome::Failed,
                                duration,
                                error_code: Some(failure.code.to_owned()),
                                error: Some(failure.message.to_owned()),
                                disk_updated: failure.disk_updated,
                            };
                            progress(RefreshProgress::SourceFinished(operation.clone()));
                            operations.push(operation);
                        }
                    }
                }
            }
        }

        let reload = if unsafe_disk_state {
            orchestrate::mark_pending_reload(
                &mut operations,
                &mut refreshed_sources,
                &mut failed_sources,
                "a source bundle could not be rolled back completely; the previous catalog snapshot remains active until disk state is repaired",
            );
            Reload::Failed {
                message: "source publication left an uncertain disk state; repair the failed bundle before reloading the catalog".to_owned(),
            }
        } else if disk_changed {
            match self.catalog.reload().await {
                Ok(_) => Reload::Reloaded,
                Err(_) => {
                    orchestrate::mark_pending_reload(
                        &mut operations,
                        &mut refreshed_sources,
                        &mut failed_sources,
                        "source files were updated, but the catalog snapshot is still the previous version",
                    );
                    Reload::Failed {
                        message: "source files were updated, but rebuilding the catalog failed; retry reload before treating them as published".to_owned(),
                    }
                }
            }
        } else {
            Reload::NotNeeded
        };
        orchestrate::order_like(&due_sources, &mut refreshed_sources);
        orchestrate::order_like(&due_sources, &mut failed_sources);
        let statuses = self
            .read_statuses(
                request.sources().to_vec(),
                request.ttl_days(),
                SystemTime::now(),
            )
            .await?;
        Ok(RefreshResult {
            ttl_days: request.ttl_days(),
            force: request.force(),
            check_only: request.check_only(),
            requested_sources: request.sources().to_vec(),
            due_sources,
            refreshed_sources,
            skipped_sources,
            failed_sources,
            statuses,
            operations,
            reload,
        })
    }

    async fn pipe_fetch_due<F, Fut>(
        &self,
        sources: &[CatalogSource],
        timeout: std::time::Duration,
        etag: Option<EntityTag>,
        fetch: F,
    ) -> Result<std::collections::BTreeMap<usize, orchestrate::TimedFetch>, RefreshError>
    where
        F: Fn(CatalogSource, Option<EntityTag>) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Result<FetchedBundle, FetchFailure>> + Send + 'static,
    {
        orchestrate::fetch_due(sources, timeout, etag, fetch).await
    }

    async fn read_statuses(
        &self,
        sources: Vec<CatalogSource>,
        ttl_days: f64,
        now: SystemTime,
    ) -> Result<Vec<SourceStatus>, RefreshError> {
        let paths = self.targets.clone();
        task::spawn_blocking(move || targets::statuses(&paths, &sources, ttl_days, now))
            .await
            .map_err(RefreshError::Join)?
    }

    async fn plan_at(
        &self,
        request: &RefreshRequest,
        now: SystemTime,
    ) -> Result<RefreshPlan, RefreshError> {
        let statuses = self
            .read_statuses(request.sources().to_vec(), request.ttl_days(), now)
            .await?;
        let due_sources = request
            .sources()
            .iter()
            .zip(&statuses)
            .filter(|(_, status)| request.force() || status.expired())
            .map(|(source, _)| *source)
            .collect::<Vec<_>>();
        let skipped_sources = request
            .sources()
            .iter()
            .filter(|source| !due_sources.contains(source))
            .copied()
            .collect();
        Ok(RefreshPlan {
            statuses,
            due_sources,
            skipped_sources,
        })
    }

    fn validate_sources(&self, sources: &[CatalogSource]) -> Result<(), RefreshError> {
        for source in sources {
            if !self.enabled.contains(*source) {
                return Err(RefreshError::InvalidSource {
                    catalog_source: source.name(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
