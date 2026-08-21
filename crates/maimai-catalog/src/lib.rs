//! 本地曲库文件适配器和不可变搜索快照。

pub mod alias;
mod enrich;
mod identity;
mod load;
mod lookup;
mod metadata;
mod normalize;
mod plate;
mod projection;
pub mod query;
mod raw;
mod search;
mod store;

pub use alias::{
    AddAliasOutcome, AddAliasRequest, AddAliasResult, AliasError, AliasKind, AliasListRequest,
    AliasListResult, AliasSong, AliasText, CanonicalName, DeleteAliasRequest, DeleteAliasResult,
    NameAliasEntry, NameAliasMutation, SongAliasEntry, SongAliasMutation, SongAliasTarget,
    SongTitle,
};
pub use load::{CatalogError, CatalogFiles, CatalogSnapshot};
pub use lookup::CatalogLookupError;
pub use metadata::{
    ChartFitStats, ChartQueryMetadata, Region, RegionAvailability, RegionOverride,
    SongQueryMetadata, SourceChartProjection, SourceKind, SourceSongFields, SourceSongProjection,
};
pub use normalize::TextNormalizer;
pub use plate::{
    PlateChart, PlateMember, PlateMemberIdentity, PlateMembers, PlateMembership, PlateName,
    PlateNameError, PlateQuery, PlateServer,
};
pub use query::{
    CatalogQuery, CatalogSort, FitLabel, InclusiveRange, MatchKind, MatchedChart, NewSongSource,
    QueryError, SearchHit, SongIdFilter, SortDirection, SortKey, SourceChartMatch,
};
pub use store::{CatalogStore, CatalogStoreError, ReloadOutcome};
