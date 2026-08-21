use maimai_core::{GroupId, QqId};
use maimai_storage::{IdentityRecord, StateStore, WaterfishIdentityProfile};
use time::OffsetDateTime;

use super::{IdentityError, IdentityQuery, MaxResults, Resolution};

/// Local identity directory backed by the persisted QQ identity snapshot.
#[derive(Clone)]
pub struct IdentityDirectory {
    store: StateStore,
}

impl IdentityDirectory {
    pub fn new(store: StateStore) -> Self {
        Self { store }
    }

    pub(crate) const fn store(&self) -> &StateStore {
        &self.store
    }

    pub async fn get_identity(
        &self,
        qq: &QqId,
        preferred_group: Option<&GroupId>,
    ) -> Result<Option<IdentityRecord>, IdentityError> {
        Ok(self.store.identity(qq, preferred_group).await?)
    }

    pub async fn resolve_identity(
        &self,
        query: &IdentityQuery,
        preferred_group: Option<&GroupId>,
        max_results: MaxResults,
    ) -> Result<Resolution, IdentityError> {
        super::resolve::resolve(self, query, preferred_group, max_results).await
    }

    pub async fn upsert_waterfish_identity(
        &self,
        qq: &QqId,
        profile: &WaterfishIdentityProfile,
        updated_at: OffsetDateTime,
    ) -> Result<(), IdentityError> {
        self.store
            .upsert_waterfish_identity(qq, profile, updated_at)
            .await?;
        Ok(())
    }
}
