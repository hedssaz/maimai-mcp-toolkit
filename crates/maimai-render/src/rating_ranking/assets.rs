use std::path::Path;

use ab_glyph::FontArc;

use crate::{RenderError, assets::load_font};

pub(super) struct RatingRankingAssets {
    pub font: FontArc,
}

impl RatingRankingAssets {
    pub(super) fn load(root: &Path) -> Result<Self, RenderError> {
        Ok(Self {
            font: load_font(
                &root.join("ShangguMonoSC-Regular.otf"),
                "rating ranking font",
            )?,
        })
    }
}
