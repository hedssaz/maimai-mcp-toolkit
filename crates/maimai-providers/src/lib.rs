//! 外部服务访问适配器。

pub mod catalog_source;
pub mod diving_fish;
pub mod diving_fish_score;
pub mod lxns_oauth;
pub mod lxns_score;
pub mod napcat;
mod raw_json;

pub use catalog_source::{
    BundleStatus, CatalogSource, CatalogSourceClient, CatalogSourceConfig, CatalogSourceError,
    CatalogSourceErrorCode, DocumentDigest, DocumentStatistics, EndpointId, EntityTag,
    SourceBundle, SourceDocument, SourceMetadata, SourceTarget,
};
pub use diving_fish::{
    AuthRequirement, DivingFishClient, DivingFishCredentials, DivingFishGame, DivingFishOperation,
    DivingFishRequest, DivingFishResponse, HttpMethod, OperationMetadata, ProviderError,
    ProviderErrorCode, QueryValue, UnknownOperation,
};
pub use diving_fish_score::{
    DivingFishB50, DivingFishChartGeneration, DivingFishPlayer, DivingFishPlayerRecords,
    DivingFishRatingEntry, DivingFishScore, DivingFishScoreClient, DivingFishScoreCounts,
    DivingFishScoreError, DivingFishScoreErrorCode, PlateVersions,
};
pub use lxns_oauth::{
    AuthorizationRequest, LxnsOAuthClient, OAuthConfig, OAuthError, OAuthErrorCode, OAuthState,
    OAuthTokens, PkceVerifier,
};
pub use lxns_score::{
    CollectionRef, FriendCode, FullCombo, FullSync, LxnsChartType, LxnsDifficulty, LxnsPlayer,
    LxnsPlayerBests, LxnsPlayerScores, LxnsScore, LxnsScoreClient, LxnsScoreConfig, LxnsScoreError,
    LxnsScoreErrorCode, LxnsSongBests, LxnsSongId, PlayerUpdate, ScoreUpload, UploadReceipt,
};
pub use napcat::{
    Friend, Group, GroupMember, NapCatClient, NapCatConfig, NapCatError, NapCatErrorCode,
    OneBotEnvelope,
};
pub use raw_json::{MAX_RAW_JSON_BYTES, ProviderEnvelope, RawJsonError, RawJsonPayload};
