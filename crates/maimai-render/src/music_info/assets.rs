use std::path::{Path, PathBuf};

use ab_glyph::FontArc;
use image::RgbaImage;

use crate::{
    RenderError,
    assets::{decode_limited, load_font},
};

pub(super) struct MusicInfoAssets {
    root: PathBuf,
    pic: PathBuf,
    pub regular: FontArc,
    pub bold: FontArc,
    pub background: RgbaImage,
}

impl MusicInfoAssets {
    pub(super) fn load(root: &Path) -> Result<Self, RenderError> {
        let pic = root.join("mai/pic");
        Ok(Self {
            root: root.to_owned(),
            pic: pic.clone(),
            regular: load_font(&root.join("Torus SemiBold.otf"), "music info numeric font")?,
            bold: load_font(
                &root.join("ResourceHanRoundedCN-Bold.ttf"),
                "music info text font",
            )?,
            background: decode_limited(
                &pic.join("song_bg.png"),
                "music info background",
                1_200,
                1_300,
                32 * 1024 * 1024,
            )?,
        })
    }

    pub(super) fn optional_pic(&self, name: &str) -> Result<Option<RgbaImage>, RenderError> {
        if !safe_asset_name(name) {
            return Ok(None);
        }
        let path = self.pic.join(name);
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(&path, "music info image", 4_096, 4_096, 64 * 1024 * 1024).map(Some)
    }

    pub(super) fn placeholder_cover(&self) -> Result<Option<RgbaImage>, RenderError> {
        let path = self.root.join("mai/cover/11000.png");
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(&path, "placeholder cover", 2_048, 2_048, 32 * 1024 * 1024).map(Some)
    }
}

fn safe_asset_name(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && Path::new(value).file_name().and_then(|name| name.to_str()) == Some(value)
}
