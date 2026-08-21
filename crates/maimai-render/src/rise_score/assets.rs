use std::path::Path;

use ab_glyph::FontArc;
use image::RgbaImage;

use super::RiseScoreRenderError;
use crate::assets::{decode_limited, load_font};

const CARD_NAMES: [&str; 5] = [
    "rise_score_basic.png",
    "rise_score_advanced.png",
    "rise_score_expert.png",
    "rise_score_master.png",
    "rise_score_remaster.png",
];
const RANK_NAMES: [&str; 14] = [
    "D", "C", "B", "BB", "BBB", "A", "AA", "AAA", "S", "Sp", "SS", "SSp", "SSS", "SSSp",
];

pub(super) struct RiseScoreAssets {
    pub regular: FontArc,
    pub bold: FontArc,
    pub title: RgbaImage,
    pub design: RgbaImage,
    pub aurora: RgbaImage,
    pub shines: RgbaImage,
    pub pattern: RgbaImage,
    pub rainbow: RgbaImage,
    pub rainbow_bottom: RgbaImage,
    pub cards: Vec<RgbaImage>,
    pub standard: RgbaImage,
    pub deluxe: RgbaImage,
    pub ranks: Vec<RgbaImage>,
}

impl RiseScoreAssets {
    pub(super) fn load(root: &Path) -> Result<Self, RiseScoreRenderError> {
        let pic = root.join("mai/pic");
        require_files(root, &pic)?;
        Ok(Self {
            regular: load_font(&root.join("Torus SemiBold.otf"), "rise score font")
                .map_err(|_| invalid("Torus SemiBold.otf"))?,
            bold: load_font(
                &root.join("ResourceHanRoundedCN-Bold.ttf"),
                "rise score bold font",
            )
            .map_err(|_| invalid("ResourceHanRoundedCN-Bold.ttf"))?,
            title: image(&pic, "title.png", 400, 120)?,
            design: image(&pic, "design.png", 1_000, 120)?,
            aurora: image(&pic, "aurora.png", 4_096, 1_024)?,
            shines: image(&pic, "bg_shines.png", 2_048, 1_024)?,
            pattern: image(&pic, "pattern.png", 1_400, 512)?,
            rainbow: image(&pic, "rainbow.png", 1_024, 512)?,
            rainbow_bottom: image(&pic, "rainbow_bottom.png", 2_048, 512)?,
            cards: load_names(&pic, &CARD_NAMES, |name| name.to_owned(), 500, 180)?,
            standard: image(&pic, "SD.png", 160, 80)?,
            deluxe: image(&pic, "DX.png", 160, 80)?,
            ranks: load_names(
                &pic,
                &RANK_NAMES,
                |name| format!("UI_TTR_Rank_{name}.png"),
                256,
                120,
            )?,
        })
    }
}

fn require_files(root: &Path, pic: &Path) -> Result<(), RiseScoreRenderError> {
    let mut paths = vec![
        (
            "Torus SemiBold.otf".to_owned(),
            root.join("Torus SemiBold.otf"),
        ),
        (
            "ResourceHanRoundedCN-Bold.ttf".to_owned(),
            root.join("ResourceHanRoundedCN-Bold.ttf"),
        ),
    ];
    for name in [
        "title.png",
        "design.png",
        "aurora.png",
        "bg_shines.png",
        "pattern.png",
        "rainbow.png",
        "rainbow_bottom.png",
        "SD.png",
        "DX.png",
    ] {
        paths.push((format!("mai/pic/{name}"), pic.join(name)));
    }
    paths.extend(CARD_NAMES.map(|name| (format!("mai/pic/{name}"), pic.join(name))));
    paths.extend(RANK_NAMES.map(|name| {
        let file = format!("UI_TTR_Rank_{name}.png");
        (format!("mai/pic/{file}"), pic.join(file))
    }));
    let missing = paths
        .into_iter()
        .filter_map(|(name, path)| (!path.is_file()).then_some(name))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(RiseScoreRenderError::AssetsRequired { missing })
    }
}

fn image(
    root: &Path,
    name: &str,
    max_width: u32,
    max_height: u32,
) -> Result<RgbaImage, RiseScoreRenderError> {
    decode_limited(
        &root.join(name),
        "rise score image",
        max_width,
        max_height,
        32 * 1024 * 1024,
    )
    .map_err(|_| invalid(&format!("mai/pic/{name}")))
}

fn invalid(name: &str) -> RiseScoreRenderError {
    RiseScoreRenderError::InvalidAsset {
        name: name.to_owned(),
    }
}

fn load_names<const N: usize>(
    root: &Path,
    names: &[&str; N],
    path: impl Fn(&str) -> String,
    max_width: u32,
    max_height: u32,
) -> Result<Vec<RgbaImage>, RiseScoreRenderError> {
    names
        .iter()
        .map(|name| image(root, &path(name), max_width, max_height))
        .collect()
}
