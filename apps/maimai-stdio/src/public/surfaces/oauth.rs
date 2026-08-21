use maimai_mcp::oauth_tools::OAuthDispatcher;

use crate::public::services::PublicServices;

pub use maimai_mcp::oauth_tools::TOOL_NAMES;

pub fn dispatcher(services: &PublicServices) -> OAuthDispatcher {
    OAuthDispatcher::new(services.oauth.clone())
}
