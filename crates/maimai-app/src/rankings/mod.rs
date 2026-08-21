mod cache;
mod error;
mod members;
mod model;
mod query;
mod refresh;

use std::{collections::HashMap, sync::Arc};

use maimai_catalog::CatalogStore;
use maimai_providers::NapCatClient;
use maimai_storage::{RankingJobError, RankingNamespace, StateStore};
use time::OffsetDateTime;
use tokio::{
    sync::{Mutex, OnceCell},
    task::JoinHandle,
};

use crate::{identity::IdentityService, score_service::PlayerScoreService};

pub use error::{RankingError, RankingErrorCode};
pub use model::{
    B50MemberRank, B50Report, B50ReportOptions, B50Row, B50Sort, CacheStatus, ContextSize,
    MaxConcurrency, MaxMembers, OutputMode, QueryDelay, RankWindow, RankingLaunch, RankingResponse,
    RefreshOptions, SongMemberRank, SongReport, SongReportOptions, SongRow, SongSort, SongTarget,
    SortOrder,
};

#[derive(Clone)]
pub struct RankingService {
    store: StateStore,
    napcat: Arc<NapCatClient>,
    identity: IdentityService,
    scores: Arc<PlayerScoreService>,
    catalog: Arc<CatalogStore>,
    clock: fn() -> OffsetDateTime,
    recovery: Arc<OnceCell<()>>,
    state: Arc<Mutex<CoordinatorState>>,
}

#[derive(Debug, Default)]
struct CoordinatorState {
    active: HashMap<(RankingNamespace, String), JoinHandle<Result<(), RankingJobError>>>,
    terminal_errors: HashMap<(RankingNamespace, String), RankingJobError>,
}

impl RankingService {
    pub fn new(
        store: StateStore,
        napcat: NapCatClient,
        identity: IdentityService,
        scores: Arc<PlayerScoreService>,
        catalog: Arc<CatalogStore>,
    ) -> Self {
        Self {
            store,
            napcat: Arc::new(napcat),
            identity,
            scores,
            catalog,
            clock: OffsetDateTime::now_utc,
            recovery: Arc::new(OnceCell::new()),
            state: Arc::new(Mutex::new(CoordinatorState::default())),
        }
    }

    pub fn napcat(&self) -> &NapCatClient {
        &self.napcat
    }

    pub fn with_clock(mut self, clock: fn() -> OffsetDateTime) -> Self {
        self.clock = clock;
        self
    }

    pub(crate) fn now(&self) -> OffsetDateTime {
        (self.clock)()
    }
}

#[cfg(test)]
mod tests;
