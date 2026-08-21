mod convert;
mod dto;
mod error;
mod filter;
mod format;
mod handler;
pub(crate) mod serialize;

#[cfg(test)]
mod tests;

pub use handler::{ScoreDeployment, ScoreQueryHandler, score_query_server};

pub const MAIN_CONTRACT_JSON: &str = include_str!("../../../../contracts/main/scores.json");
pub const PUBLIC_CONTRACT_JSON: &str = include_str!("../../../../contracts/public/scores.json");
pub const TOOL_NAMES: [&str; 7] = [
    "query_b50",
    "query_b50_batch",
    "query_computed_b50",
    "query_maimai_song_score",
    "query_maimai_player_records",
    "list_diving_fish_apis",
    "diving_fish_api",
];
