use std::{collections::BTreeMap, future::Future, sync::Arc, time::Duration};

use maimai_providers::{CatalogSource, EntityTag};
use tokio::{sync::Semaphore, task::JoinSet, time};

use super::{
    error::RefreshError,
    fetch::{FetchFailure, FetchedBundle},
    model::{OperationOutcome, OperationSummary},
};

const MAX_CONCURRENT_FETCHES: usize = 4;

pub(super) struct TimedFetch {
    pub(super) result: Result<FetchedBundle, FetchFailure>,
    pub(super) elapsed: Duration,
}

pub(super) async fn fetch_due<F, Fut>(
    sources: &[CatalogSource],
    timeout: Duration,
    etag: Option<EntityTag>,
    fetch: F,
) -> Result<BTreeMap<usize, TimedFetch>, RefreshError>
where
    F: Fn(CatalogSource, Option<EntityTag>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<FetchedBundle, FetchFailure>> + Send + 'static,
{
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES));
    let mut tasks = JoinSet::new();
    for (index, source) in sources.iter().copied().enumerate() {
        let semaphore = Arc::clone(&semaphore);
        let fetch = fetch.clone();
        let etag = (source == CatalogSource::DivingFish)
            .then(|| etag.clone())
            .flatten();
        tasks.spawn(async move {
            let started = std::time::Instant::now();
            let permit = semaphore.acquire_owned().await.map_err(|_| FetchFailure {
                code: "COORDINATOR_CLOSED".to_owned(),
                message: "source refresh coordinator closed unexpectedly".to_owned(),
            });
            let result = match permit {
                Ok(_permit) => match time::timeout(timeout, fetch(source, etag)).await {
                    Ok(result) => result,
                    Err(_) => Err(FetchFailure {
                        code: "TIMEOUT".to_owned(),
                        message: "source refresh timed out".to_owned(),
                    }),
                },
                Err(error) => Err(error),
            };
            (
                index,
                TimedFetch {
                    result,
                    elapsed: started.elapsed(),
                },
            )
        });
    }
    let mut results = BTreeMap::new();
    while let Some(joined) = tasks.join_next().await {
        let (index, fetched) = joined.map_err(RefreshError::Join)?;
        results.insert(index, fetched);
    }
    Ok(results)
}

pub(super) fn failed_operation(
    source: CatalogSource,
    duration: Duration,
    failure: &FetchFailure,
) -> OperationSummary {
    OperationSummary {
        source,
        outcome: OperationOutcome::Failed,
        duration,
        error_code: Some(failure.code.clone()),
        error: Some(failure.message.clone()),
        disk_updated: false,
    }
}

pub(super) fn order_like(order: &[CatalogSource], values: &mut Vec<CatalogSource>) {
    values.sort_by_key(|value| order.iter().position(|source| source == value));
    values.dedup();
}

pub(super) fn mark_pending_reload(
    operations: &mut [OperationSummary],
    refreshed_sources: &mut Vec<CatalogSource>,
    failed_sources: &mut Vec<CatalogSource>,
    message: &str,
) {
    for operation in operations {
        if matches!(
            operation.outcome,
            OperationOutcome::Updated | OperationOutcome::NotModified
        ) {
            operation.outcome = OperationOutcome::DiskUpdatedPendingReload;
            operation.error_code = Some("CATALOG_RELOAD_FAILED".to_owned());
            operation.error = Some(message.to_owned());
            if !failed_sources.contains(&operation.source) {
                failed_sources.push(operation.source);
            }
        }
    }
    refreshed_sources.clear();
}
