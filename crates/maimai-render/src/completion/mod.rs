mod assets;
mod cells;
mod model;
mod plate;
mod progress;
mod rating;

use std::{io::Cursor, path::Path};

use image::{ImageEncoder, RgbaImage, codecs::png::PngEncoder};

use crate::{
    CoverResolver, RenderError,
    score_list::{ScoreListRenderError, assets::ScoreListAssets},
};

use assets::CompletionAssets;
pub use model::{
    CompletionRenderedPng, CompletionState, FullComboStatus, FullSyncStatus, LevelProgressView,
    PlateChartState, PlateMemberView, PlateTableKind, PlateTableView, ProgressPage, RatingAllClear,
    RatingConstantGroup, RatingScoreCell, RatingStatistics, RatingTableMode, RatingTableView,
    ScoreCardCell,
};

pub struct CompletionRenderer {
    assets: CompletionAssets,
    score_assets: ScoreListAssets,
    covers: CoverResolver,
}

impl CompletionRenderer {
    pub fn new(
        static_root: impl AsRef<Path>,
        cover_cache_root: impl AsRef<Path>,
    ) -> Result<Self, RenderError> {
        let static_root = static_root.as_ref();
        Ok(Self {
            assets: CompletionAssets::load(static_root)?,
            score_assets: ScoreListAssets::load(static_root).map_err(completion_asset_error)?,
            covers: CoverResolver::new(static_root, cover_cache_root.as_ref()),
        })
    }
}

fn completion_asset_error(error: ScoreListRenderError) -> RenderError {
    let missing = match error {
        ScoreListRenderError::AssetsRequired { missing } => missing,
        ScoreListRenderError::InvalidAsset { name } => vec![name],
        ScoreListRenderError::Render(error) => return error,
    };
    RenderError::AssetsRequired {
        style: "completion",
        missing,
    }
}

pub(crate) fn encode(image: RgbaImage) -> Result<(Vec<u8>, u32, u32), RenderError> {
    let (width, height) = image.dimensions();
    let mut bytes = Vec::new();
    PngEncoder::new(Cursor::new(&mut bytes))
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(RenderError::PngEncode)?;
    Ok((bytes, width, height))
}

#[cfg(test)]
mod tests;
