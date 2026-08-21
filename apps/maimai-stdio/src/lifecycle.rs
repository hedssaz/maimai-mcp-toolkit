use std::sync::Arc;

use maimai_app::{
    catalog_refresh::job::CatalogRefreshJobs, identity::IdentityService, rankings::RankingService,
};
use maimai_storage::StateStore;
use thiserror::Error;
use time::OffsetDateTime;

pub(crate) struct SharedLifecycle {
    rankings: RankingService,
    identity: IdentityService,
    catalog_jobs: Arc<CatalogRefreshJobs>,
    state: StateStore,
}

impl SharedLifecycle {
    pub(crate) const fn new(
        rankings: RankingService,
        identity: IdentityService,
        catalog_jobs: Arc<CatalogRefreshJobs>,
        state: StateStore,
    ) -> Self {
        Self {
            rankings,
            identity,
            catalog_jobs,
            state,
        }
    }

    pub(crate) async fn shutdown(self) -> Result<(), SharedLifecycleError> {
        let now = OffsetDateTime::now_utc();
        let mut first = None;
        if let Err(error) = self.rankings.shutdown(now).await {
            first = Some(SharedLifecycleError::Rankings(error));
        }
        if let Err(error) = self.identity.shutdown(now).await {
            first.get_or_insert(SharedLifecycleError::Identity(error));
        }
        if let Err(error) = self.catalog_jobs.shutdown().await {
            first.get_or_insert(SharedLifecycleError::Catalog(error));
        }
        self.state.close().await;
        first.map_or(Ok(()), Err)
    }
}

#[derive(Debug, Error)]
pub enum SharedLifecycleError {
    #[error(transparent)]
    Rankings(maimai_app::rankings::RankingError),
    #[error(transparent)]
    Identity(maimai_app::identity::IdentityError),
    #[error(transparent)]
    Catalog(maimai_app::catalog_refresh::job::RefreshJobError),
}
