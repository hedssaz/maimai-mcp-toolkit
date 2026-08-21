mod client;
mod codec;
mod config;
mod endpoint;
mod error;
mod model;
mod redaction;
mod request;
mod write;

const MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;

pub use client::LxnsScoreClient;
pub use config::{DEFAULT_LXNS_SCORE_BASE_URL, LxnsScoreConfig};
pub use endpoint::LxnsScoreEndpoint;
pub use error::{LxnsScoreError, LxnsScoreErrorCode};
pub use model::{
    CollectionRef, FriendCode, FullCombo, FullSync, LxnsChartType, LxnsDifficulty, LxnsPlayer,
    LxnsPlayerBests, LxnsPlayerScores, LxnsScore, LxnsSongBests, LxnsSongId,
};
pub use write::{PlayerUpdate, ScoreUpload, UploadReceipt};

#[cfg(test)]
mod tests;
