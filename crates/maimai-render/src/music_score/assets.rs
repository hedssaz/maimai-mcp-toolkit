use std::path::{Path, PathBuf};

use ab_glyph::FontArc;
use image::RgbaImage;

use crate::{
    RenderError,
    assets::{decode_limited, load_font},
};

pub(super) struct MusicScoreAssets {
    root: PathBuf,
    pic: PathBuf,
    pub regular: FontArc,
    pub bold: FontArc,
    pub background: RgbaImage,
}

impl MusicScoreAssets {
    pub fn load(root: &Path) -> Result<Self, RenderError> {
        let pic = root.join("mai/pic");
        Ok(Self {
            root: root.to_owned(),
            pic: pic.clone(),
            regular: load_font(&root.join("Torus SemiBold.otf"), "music score numeric font")?,
            bold: load_font(
                &root.join("ResourceHanRoundedCN-Bold.ttf"),
                "music score text font",
            )?,
            background: decode_limited(
                &pic.join("info_bg.png"),
                "music score background",
                1_200,
                900,
                32 * 1024 * 1024,
            )?,
        })
    }

    pub fn pic(&self, name: &str) -> Result<RgbaImage, RenderError> {
        decode_limited(
            &self.pic.join(name),
            "music score image",
            4_096,
            4_096,
            64 * 1024 * 1024,
        )
    }

    pub fn optional_pic(&self, name: &str) -> Result<Option<RgbaImage>, RenderError> {
        if !safe_asset_name(name) {
            return Ok(None);
        }
        let path = self.pic.join(name);
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(&path, "music score image", 4_096, 4_096, 64 * 1024 * 1024).map(Some)
    }

    pub fn placeholder_cover(&self) -> Result<Option<RgbaImage>, RenderError> {
        let path = self.root.join("mai/cover/11000.png");
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(
            &path,
            "music score placeholder cover",
            2_048,
            2_048,
            32 * 1024 * 1024,
        )
        .map(Some)
    }
}

fn safe_asset_name(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && Path::new(value).file_name().and_then(|name| name.to_str()) == Some(value)
}
