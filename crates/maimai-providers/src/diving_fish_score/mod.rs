mod client;
mod decode;
mod error;
mod model;

pub use client::DivingFishScoreClient;
pub use error::{DivingFishScoreError, DivingFishScoreErrorCode};
pub use model::{
    DivingFishB50, DivingFishChartGeneration, DivingFishPlayer, DivingFishPlayerRecords,
    DivingFishRatingEntry, DivingFishScore, DivingFishScoreCounts, PlateVersions,
};

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
