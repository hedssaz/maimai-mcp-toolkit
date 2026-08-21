use maimai_app::identity::{DEFAULT_RESET_HOUR_UTC, IdentityError, ResetHour};
use maimai_mcp::identity_tools::{DisplayOffset, IdentityDispatcher};

use crate::public::services::PublicServices;

pub const TOOL_NAMES: [&str; 5] = [
    "refresh_qq_identity_cache",
    "qq_identity_cache_status",
    "qq_identity_job_status",
    "resolve_qq_identity",
    "get_qq_identity",
];

pub fn dispatcher(services: &PublicServices) -> Result<IdentityDispatcher, IdentityError> {
    Ok(IdentityDispatcher::with_display_offset(
        services.identity.clone(),
        ResetHour::new(DEFAULT_RESET_HOUR_UTC)?,
        DisplayOffset::new(services.display_offset),
    ))
}
