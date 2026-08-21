use std::path::{Path, PathBuf};

use ab_glyph::FontArc;
use image::RgbaImage;

use crate::{
    RenderError,
    assets::{decode_limited, load_font},
};

pub(crate) struct CompletionAssets {
    root: PathBuf,
    pub(crate) regular: FontArc,
    pub(crate) bold: FontArc,
    pub(crate) mono: FontArc,
}

impl CompletionAssets {
    pub(crate) fn load(root: &Path) -> Result<Self, RenderError> {
        Ok(Self {
            root: root.to_owned(),
            regular: load_font(&root.join("Torus SemiBold.otf"), "completion regular font")?,
            bold: load_font(
                &root.join("ResourceHanRoundedCN-Bold.ttf"),
                "completion bold font",
            )?,
            mono: load_font(
                &root.join("ShangguMonoSC-Regular.otf"),
                "completion text panel font",
            )?,
        })
    }

    pub(crate) fn plate_template(&self, name: &str) -> Result<Option<RgbaImage>, RenderError> {
        if !safe_asset_stem(name) {
            return Ok(None);
        }
        let path = self.root.join("mai/plate").join(format!("{name}.png"));
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(&path, "plate template", 1_400, 16_000, 256 * 1024 * 1024).map(Some)
    }

    pub(crate) fn plate_title(&self, name: &str) -> Result<Option<RgbaImage>, RenderError> {
        if !safe_asset_stem(name) {
            return Ok(None);
        }
        let path = self.root.join("mai/plate").join(format!("{name}.png"));
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(&path, "plate title", 4_096, 1_024, 64 * 1024 * 1024).map(Some)
    }

    pub(crate) fn rating_template(&self, level: &str) -> Result<Option<RgbaImage>, RenderError> {
        if !safe_asset_stem(level) {
            return Ok(None);
        }
        let path = self.root.join("mai/rating").join(format!("{level}.png"));
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(&path, "rating template", 1_400, 16_000, 256 * 1024 * 1024).map(Some)
    }

    pub(crate) fn pic(&self, name: &str) -> Result<RgbaImage, RenderError> {
        if !safe_asset_file(name) {
            return Err(RenderError::invalid(
                "completion.asset",
                "invalid asset name",
            ));
        }
        decode_limited(
            &self.root.join("mai/pic").join(name),
            "completion image",
            4_096,
            4_096,
            64 * 1024 * 1024,
        )
    }

    pub(crate) fn optional_pic(&self, name: &str) -> Result<Option<RgbaImage>, RenderError> {
        if !safe_asset_file(name) {
            return Ok(None);
        }
        let path = self.root.join("mai/pic").join(name);
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(&path, "completion image", 4_096, 4_096, 64 * 1024 * 1024).map(Some)
    }
}

fn safe_asset_stem(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 96
        && value.chars().all(|character| {
            !character.is_control()
                && !matches!(character, '/' | '\\' | ':' | '\0')
                && character != '.'
        })
}

fn safe_asset_file(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && Path::new(value).file_name().and_then(|name| name.to_str()) == Some(value)
}

#[cfg(test)]
mod tests {
    use super::safe_asset_stem;

    #[test]
    fn plate_asset_name_never_becomes_a_path() {
        assert!(safe_asset_stem("jp_丸"));
        assert!(!safe_asset_stem("../../secret"));
        assert!(!safe_asset_stem("custom/name"));
    }
}
