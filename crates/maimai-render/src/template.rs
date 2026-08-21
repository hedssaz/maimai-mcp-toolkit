use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
};

use ab_glyph::FontArc;
use image::{
    ColorType, ImageEncoder, RgbaImage,
    codecs::png::PngEncoder,
    imageops::{FilterType, overlay, resize},
};

use crate::{
    RenderError, RenderMetadata, RenderedPng, TemplateStyle,
    assets::{decode_limited, load_font},
};

pub(crate) const YUZU_ROOT_ASSETS: &[&str] =
    &["ResourceHanRoundedCN-Bold.ttf", "Torus SemiBold.otf"];
pub(crate) const YUZU_PIC_ASSETS: &[&str] = &[
    "b50_bg.png",
    "b50_score_basic.png",
    "b50_score_advanced.png",
    "b50_score_expert.png",
    "b50_score_master.png",
    "b50_score_remaster.png",
    "logo.png",
    "Name.png",
    "UI_CMN_DXRating_10.png",
    "UI_CMN_DXRating_11.png",
    "UI_CMN_Shougou_Rainbow.png",
    "UI_DNM_DaniPlate_00.png",
    "UI_FBR_Class_00.png",
    "UI_Icon_309503.png",
    "UI_Plate_300501.png",
    "UI_TTR_Rank_SSS.png",
    "UI_TTR_Rank_SSSp.png",
    "SD.png",
    "DX.png",
];
pub(crate) const MAIBOT_PIC_ASSETS: &[&str] = &[
    "UI_TTR_BG_Base_Plus.png",
    "UI_CMN_TabTitle_MaimaiTitle_Ver214.png",
    "UI_CMN_DXRating_S_10.png",
    "UI_CMN_Name_DX.png",
    "UI_TST_PlateMask.png",
    "UI_CMN_Shougou_Rainbow.png",
    "UI_RSL_MBase_Parts_01.png",
    "UI_RSL_MBase_Parts_02.png",
    "UI_GAM_Rank_SSSp.png",
];

pub(crate) struct TemplateAssets {
    pub(crate) root: PathBuf,
    pub(crate) pic: PathBuf,
    pub(crate) regular: FontArc,
    pub(crate) bold: FontArc,
    images: HashMap<String, RgbaImage>,
}

impl TemplateAssets {
    pub(crate) fn yuzu(root: &Path) -> Result<Self, RenderError> {
        require(root, YUZU_ROOT_ASSETS, YUZU_PIC_ASSETS, TemplateStyle::Yuzu)?;
        let pic = root.join("mai/pic");
        Ok(Self {
            root: root.to_owned(),
            images: load_images(&pic, YUZU_PIC_ASSETS)?,
            pic,
            regular: load_font(&root.join("Torus SemiBold.otf"), "yuzu regular font")?,
            bold: load_font(
                &root.join("ResourceHanRoundedCN-Bold.ttf"),
                "yuzu bold font",
            )?,
        })
    }

    pub(crate) fn maibot(
        root: &Path,
        regular: FontArc,
        bold: FontArc,
    ) -> Result<Self, RenderError> {
        require(root, &[], MAIBOT_PIC_ASSETS, TemplateStyle::Maibot)?;
        let pic = root.join("mai/pic");
        Ok(Self {
            root: root.to_owned(),
            images: load_images(&pic, MAIBOT_PIC_ASSETS)?,
            pic,
            regular,
            bold,
        })
    }

    pub(crate) fn pic(&self, name: &str) -> Result<RgbaImage, RenderError> {
        self.images
            .get(name)
            .cloned()
            .ok_or_else(|| RenderError::invalid_asset("template image", &self.pic.join(name)))
    }

    pub(crate) fn optional_pic(&self, name: &str) -> Result<Option<RgbaImage>, RenderError> {
        if let Some(image) = self.images.get(name) {
            return Ok(Some(image.clone()));
        }
        let path = self.pic.join(name);
        if !path.is_file() {
            return Ok(None);
        }
        decode_limited(&path, "template image", 4_096, 4_096, 128 * 1024 * 1024).map(Some)
    }
}

fn load_images(pic: &Path, names: &[&str]) -> Result<HashMap<String, RgbaImage>, RenderError> {
    names
        .iter()
        .map(|name| {
            decode_limited(
                &pic.join(name),
                "template image",
                4_096,
                4_096,
                128 * 1024 * 1024,
            )
            .map(|image| ((*name).to_owned(), image))
        })
        .collect()
}

pub(crate) fn paste(
    target: &mut RgbaImage,
    source: &RgbaImage,
    x: i64,
    y: i64,
    size: Option<(u32, u32)>,
) {
    if let Some((width, height)) = size {
        overlay(
            target,
            &resize(source, width, height, FilterType::Lanczos3),
            x,
            y,
        );
    } else {
        overlay(target, source, x, y);
    }
}

pub(crate) fn encoded(
    image: &RgbaImage,
    card_count: usize,
    missing_covers: Vec<crate::MissingCover>,
) -> Result<RenderedPng, RenderError> {
    let mut bytes = Vec::new();
    PngEncoder::new(Cursor::new(&mut bytes))
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ColorType::Rgba8.into(),
        )
        .map_err(RenderError::PngEncode)?;
    Ok(RenderedPng {
        bytes,
        metadata: RenderMetadata {
            width: image.width(),
            height: image.height(),
            card_count,
            missing_covers,
        },
    })
}

fn require(
    root: &Path,
    root_assets: &[&str],
    pic_assets: &[&str],
    style: TemplateStyle,
) -> Result<(), RenderError> {
    let mut missing = root_assets
        .iter()
        .filter(|name| !root.join(name).is_file())
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    missing.extend(
        pic_assets
            .iter()
            .filter(|name| !root.join("mai/pic").join(name).is_file())
            .map(|name| format!("mai/pic/{name}")),
    );
    if missing.is_empty() {
        Ok(())
    } else {
        missing.sort();
        Err(RenderError::assets_required(style, missing))
    }
}
