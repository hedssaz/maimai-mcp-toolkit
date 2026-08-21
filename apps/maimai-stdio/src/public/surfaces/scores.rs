use maimai_mcp::{
    PairDispatcher,
    score_queries::{ScoreDeployment, ScoreQueryHandler},
    score_settings::ScoreSettingsHandler,
};

use crate::public::services::PublicServices;

pub const QUERY_TOOL_NAMES: [&str; 7] = maimai_mcp::score_queries::TOOL_NAMES;
#[cfg(test)]
pub const PUBLIC_TOOL_NAMES: [&str; 10] = [
    "query_b50",
    "query_b50_batch",
    "query_computed_b50",
    "query_maimai_song_score",
    "query_maimai_player_records",
    "list_diving_fish_apis",
    "diving_fish_api",
    "bind_developer_token",
    "developer_token_status",
    "clear_developer_token",
];
pub type ScoresDispatcher = PairDispatcher<ScoreQueryHandler, ScoreSettingsHandler>;

pub fn dispatcher(services: &PublicServices) -> ScoresDispatcher {
    PairDispatcher::new(
        &QUERY_TOOL_NAMES,
        ScoreQueryHandler::new(
            services.scores.clone(),
            services.identity.directory().clone(),
            services.score_settings.clone(),
            services.catalog.clone(),
            services.diving_fish.clone(),
            ScoreDeployment::Public,
        ),
        ScoreSettingsHandler::new(services.score_settings.clone()),
    )
}
