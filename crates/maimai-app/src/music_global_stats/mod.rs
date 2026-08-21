mod error;
mod model;
mod resolve;
mod service;

pub use error::MusicGlobalStatsError;
pub use model::{
    MusicGlobalStatsBatchResult, MusicGlobalStatsDifficulty, MusicGlobalStatsFailureKind,
    MusicGlobalStatsImage, MusicGlobalStatsItemError, MusicGlobalStatsRequest,
};
pub use service::MusicGlobalStatsService;

#[cfg(test)]
mod tests;
