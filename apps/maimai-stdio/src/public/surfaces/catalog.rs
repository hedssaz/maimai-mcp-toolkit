use maimai_mcp::catalog_tools::CatalogDispatcher;

use crate::public::services::PublicServices;

pub const TOOL_NAMES: [&str; 13] = [
    "search_maimai_songs",
    "batch_search_maimai_songs",
    "add_maimai_alias",
    "delete_maimai_alias",
    "list_maimai_aliases",
    "refresh_maimai_sources",
    "refresh_maimai_sources_job_status",
    "random_maimai_songs",
    "today_maimai",
    "list_maimai_songs_by_id",
    "list_maimai_versions",
    "score_counts",
    "find_score_combinations",
];

pub fn dispatcher(services: &PublicServices) -> CatalogDispatcher {
    CatalogDispatcher::new(
        services.catalog.clone(),
        services.catalog_refresh.clone(),
        services.catalog_jobs.clone(),
    )
}
