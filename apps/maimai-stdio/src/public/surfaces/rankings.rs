use maimai_mcp::ranking_tools::RankingDispatcher;

use crate::public::services::PublicServices;

pub use maimai_mcp::ranking_tools::TOOL_NAMES;

pub fn dispatcher(services: &PublicServices) -> RankingDispatcher {
    RankingDispatcher::new(services.rankings.clone(), services.display_offset)
}
