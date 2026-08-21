mod directory;
mod error;
mod group;
mod job;
mod model;
mod refresh;
mod resolve;
mod status;

use std::sync::Arc;

use maimai_providers::{NapCatClient, NapCatConfig};
use maimai_storage::{StateStore, WaterfishIdentityProfile};
use time::OffsetDateTime;
use tokio::sync::Mutex;

pub use directory::IdentityDirectory;
pub use error::IdentityError;
pub use group::{GroupMemberPolicy, IdentityGroupMember};
pub use model::{
    CacheStatus, GroupDelay, IdentityField, IdentityMatch, IdentityQuery, MaxGroups, MaxResults,
    NoCache, RefreshJobLaunch, RefreshJobRequest, RefreshOptions, RefreshPolicy, RefreshReport,
    ResetHour, Resolution,
};
pub use status::DEFAULT_RESET_HOUR_UTC;

use maimai_core::QqId;

#[derive(Clone)]
pub struct IdentityService {
    pub(crate) directory: IdentityDirectory,
    pub(crate) napcat: Arc<NapCatClient>,
    pub(crate) refresh_state: Arc<Mutex<job::RefreshState>>,
}

impl IdentityService {
    pub fn new(store: StateStore, napcat: NapCatClient) -> Self {
        Self {
            directory: IdentityDirectory::new(store),
            napcat: Arc::new(napcat),
            refresh_state: Arc::new(Mutex::new(job::RefreshState::default())),
        }
    }

    pub fn napcat_config(&self) -> &NapCatConfig {
        self.napcat.config()
    }

    pub const fn directory(&self) -> &IdentityDirectory {
        &self.directory
    }

    pub async fn upsert_waterfish_identity(
        &self,
        qq: &QqId,
        profile: &WaterfishIdentityProfile,
        updated_at: OffsetDateTime,
    ) -> Result<(), IdentityError> {
        self.directory
            .upsert_waterfish_identity(qq, profile, updated_at)
            .await
    }
}
