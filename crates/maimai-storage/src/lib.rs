//! Async SQLite persistence for player profiles, stable score records, and source choices.

mod catalog_refresh_job;
mod diving_fish_credentials;
mod error;
mod identity;
mod model;
mod oauth;
mod player_cache;
mod rankings;
mod schema;
mod store;

pub use catalog_refresh_job::{
    CatalogRefreshJob, CatalogRefreshJobId, CatalogRefreshJobOutcome, CatalogRefreshJobSource,
    CatalogRefreshJobStart, CatalogRefreshJobStatus, CatalogRefreshJobStore, CatalogRefreshOwner,
    CatalogRefreshOwnerClaim, CatalogRefreshSourceStatus, CatalogRefreshSourceUpdate,
    CatalogRefreshTerminalUpdate,
};
pub use diving_fish_credentials::{DeveloperTokenMetadata, DivingFishDeveloperToken};
pub use error::StorageError;
pub use identity::{
    IdentityGroupMembership, IdentityGroupSnapshot, IdentityJob, IdentityJobError,
    IdentityJobErrorCode, IdentityJobProgress, IdentityJobStart, IdentityJobStatus,
    IdentityMetadata, IdentityRecord, IdentityRefreshReason, IdentitySnapshot,
    IdentitySnapshotMember, IdentityStats, WaterfishIdentityProfile,
};
pub use model::{
    FullScoreSnapshot, FullScoreSnapshotWriteOutcome, LegacyImportReport, PlayerProfile,
    PlayerRecord,
};
pub use oauth::{
    AuthorizationClaim, AuthorizationClaimResult, NewOAuthAuthorization, NewOAuthToken,
    OAuthAuthorization, OAuthCasResult, OAuthConfirmResult, OAuthContext, OAuthPendingPoke,
    OAuthTokenRecord,
};
pub use player_cache::{B50CacheWriteOutcome, PlayerB50Snapshot};
pub use rankings::{
    B50Section, CachedB50Chart, CachedB50Entry, CachedChart, CachedExactRatio, CachedFitIndex,
    CachedFitIndexLabel, CachedFitIndexSection, CachedPlayer, RankingCache, RankingJob,
    RankingJobError, RankingJobErrorCode, RankingJobProgress, RankingJobStart, RankingJobStatus,
    RankingMember, RankingNamespace, RankingRefreshReason, RankingSnapshot, RankingSnapshotData,
};
pub use store::StateStore;
