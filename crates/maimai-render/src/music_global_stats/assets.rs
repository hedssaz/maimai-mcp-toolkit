use std::path::Path;

use ab_glyph::FontArc;

use crate::{RenderError, assets::load_font};

pub(super) struct MusicGlobalStatsAssets {
    pub regular: FontArc,
    pub bold: FontArc,
}

impl MusicGlobalStatsAssets {
    pub(super) fn load(root: &Path) -> Result<Self, RenderError> {
        Ok(Self {
            regular: load_font(
                &root.join("Torus SemiBold.otf"),
                "music global stats numeric font",
            )?,
            bold: load_font(
                &root.join("ResourceHanRoundedCN-Bold.ttf"),
                "music global stats text font",
            )?,
        })
    }
}
