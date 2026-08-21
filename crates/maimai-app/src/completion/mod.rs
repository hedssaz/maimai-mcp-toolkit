mod error;
mod model;
mod plate;
mod plate_progress;
mod progress;
mod rating;
mod records;
mod service;

pub use error::CompletionError;
pub use model::{
    AchievementTarget, CompletionCapabilities, CompletionIdentity, CompletionImage,
    CompletionItemError, CompletionResponse, CompletionTarget, LevelProgressRequest,
    MAX_COMPLETION_BATCH_ITEMS, PlateBatchResult, PlateProgressBatchResult, PlateProgressItem,
    PlateProgressOutput, PlateSpec, ProgressCategory, RatingTableImage, RatingTableRequest,
};
pub use service::CompletionService;

#[cfg(test)]
mod tests;
