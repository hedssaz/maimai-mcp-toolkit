mod error;
mod model;
pub(crate) mod resolve;
mod service;

pub use error::MusicInfoError;
pub use model::{
    KnownMusicMetadata, MAX_MUSIC_INFO_BATCH_ITEMS, MusicInfoBatchResult, MusicInfoChartType,
    MusicInfoImage, MusicInfoItemError, MusicInfoRequest, ResolvedSongHint,
};
pub use service::MusicInfoService;

#[cfg(test)]
mod tests;
