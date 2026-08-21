//! NapCat OneBot 11 HTTP 查询适配器。

mod client;
mod config;
mod error;
mod models;
mod redaction;

pub use client::NapCatClient;
pub use config::{DEFAULT_NAPCAT_BASE_URL, NapCatConfig};
pub use error::{NapCatError, NapCatErrorCode};
pub use models::{Friend, Group, GroupMember, OneBotEnvelope};

const MAX_RESPONSE_BODY_BYTES: usize = 16 * 1024 * 1024;

#[cfg(test)]
mod tests;
