use std::{fs, path::Path};

use ab_glyph::FontArc;

use super::geometry::{RegionFeature, load_features};
use crate::{RenderError, assets::load_font};

const MAX_GEOJSON_BYTES: u64 = 4 * 1024 * 1024;

pub struct RegionHeatmapAssets {
    pub(super) regular: FontArc,
    pub(super) bold: FontArc,
    pub(super) features: Vec<RegionFeature>,
}

impl RegionHeatmapAssets {
    pub fn load(
        geojson_path: impl AsRef<Path>,
        regular_font: impl AsRef<Path>,
        bold_font: impl AsRef<Path>,
    ) -> Result<Self, RenderError> {
        let geojson_path = geojson_path.as_ref();
        let metadata = fs::metadata(geojson_path)
            .map_err(|source| RenderError::asset_read("region GeoJSON", geojson_path, source))?;
        if !metadata.is_file() || metadata.len() > MAX_GEOJSON_BYTES {
            return Err(RenderError::invalid_asset("region GeoJSON", geojson_path));
        }
        let source = fs::read(geojson_path)
            .map_err(|error| RenderError::asset_read("region GeoJSON", geojson_path, error))?;
        let features = load_features(&source)
            .map_err(|_| RenderError::invalid_asset("region GeoJSON", geojson_path))?;
        Ok(Self {
            regular: load_font(regular_font.as_ref(), "region regular font")?,
            bold: load_font(bold_font.as_ref(), "region bold font")?,
            features,
        })
    }
}
