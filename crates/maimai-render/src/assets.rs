use std::{fs, path::PathBuf};

use ab_glyph::FontArc;
use image::{ImageError, ImageReader, Limits, RgbaImage};

use crate::RenderError;

const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyAssets {
    regular_font: PathBuf,
    bold_font: PathBuf,
    background: Option<PathBuf>,
}

impl LegacyAssets {
    pub fn new(regular_font: impl Into<PathBuf>, bold_font: impl Into<PathBuf>) -> Self {
        Self {
            regular_font: regular_font.into(),
            bold_font: bold_font.into(),
            background: None,
        }
    }

    pub fn with_background(mut self, background: impl Into<PathBuf>) -> Self {
        self.background = Some(background.into());
        self
    }

    pub fn regular_font(&self) -> &std::path::Path {
        &self.regular_font
    }

    pub fn bold_font(&self) -> &std::path::Path {
        &self.bold_font
    }

    pub fn background(&self) -> Option<&std::path::Path> {
        self.background.as_deref()
    }

    pub(crate) fn load(self) -> Result<LoadedAssets, RenderError> {
        let regular_font = load_font(&self.regular_font, "regular font")?;
        let bold_font = load_font(&self.bold_font, "bold font")?;
        let background = self
            .background
            .as_ref()
            .map(|path| decode_limited(path, "background", 4_096, 4_096, 128 * 1024 * 1024))
            .transpose()?;
        Ok(LoadedAssets {
            regular_font,
            bold_font,
            background,
        })
    }
}

pub(crate) fn load_cover(path: &std::path::Path) -> Result<Option<RgbaImage>, RenderError> {
    let Ok(mut reader) = ImageReader::open(path) else {
        return Ok(None);
    };
    reader.limits(decode_limits(2_048, 2_048, 32 * 1024 * 1024));
    match reader.decode() {
        Ok(image) => Ok(Some(image.to_rgba8())),
        Err(ImageError::Limits(_)) => Err(RenderError::invalid_asset("cover", path)),
        Err(_) => Ok(None),
    }
}

pub(crate) struct LoadedAssets {
    pub(crate) regular_font: FontArc,
    pub(crate) bold_font: FontArc,
    pub(crate) background: Option<RgbaImage>,
}

pub(crate) fn load_font(
    path: &std::path::Path,
    kind: &'static str,
) -> Result<FontArc, RenderError> {
    let metadata =
        fs::metadata(path).map_err(|source| RenderError::asset_read(kind, path, source))?;
    if !metadata.is_file() || metadata.len() > MAX_FONT_BYTES {
        return Err(RenderError::invalid_asset(kind, path));
    }
    let bytes = fs::read(path).map_err(|source| RenderError::asset_read(kind, path, source))?;
    FontArc::try_from_vec(bytes).map_err(|_| RenderError::invalid_asset(kind, path))
}

pub(crate) fn decode_limited(
    path: &std::path::Path,
    kind: &'static str,
    max_width: u32,
    max_height: u32,
    max_alloc: u64,
) -> Result<RgbaImage, RenderError> {
    let mut reader =
        ImageReader::open(path).map_err(|source| RenderError::asset_read(kind, path, source))?;
    reader.limits(decode_limits(max_width, max_height, max_alloc));
    reader
        .decode()
        .map(|image| image.to_rgba8())
        .map_err(|_| RenderError::invalid_asset(kind, path))
}

fn decode_limits(max_width: u32, max_height: u32, max_alloc: u64) -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(max_width);
    limits.max_image_height = Some(max_height);
    limits.max_alloc = Some(max_alloc);
    limits
}
