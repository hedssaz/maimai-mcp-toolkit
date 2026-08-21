use std::path::Path;

use ab_glyph::FontArc;
use image::RgbaImage;

use super::ScoreListRenderError;
use crate::assets::{decode_limited, load_font};

const CARD_NAMES: [&str; 5] = [
    "b50_score_basic.png",
    "b50_score_advanced.png",
    "b50_score_expert.png",
    "b50_score_master.png",
    "b50_score_remaster.png",
];
const RANK_NAMES: [&str; 14] = [
    "D", "C", "B", "BB", "BBB", "A", "AA", "AAA", "S", "Sp", "SS", "SSp", "SSS", "SSSp",
];
const COMBO_NAMES: [&str; 4] = ["FC", "FCp", "AP", "APp"];
const SYNC_NAMES: [&str; 5] = ["Sync", "FS", "FSp", "FSD", "FSDp"];

pub(crate) struct ScoreListAssets {
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
    pub combo: Vec<RgbaImage>,
    pub sync: Vec<RgbaImage>,
    pub dx_gauges: Vec<RgbaImage>,
}

impl ScoreListAssets {
    pub(crate) fn load(root: &Path) -> Result<Self, ScoreListRenderError> {
        let pic = root.join("mai/pic");
        require_files(root, &pic)?;
        Ok(Self {
            regular: load_font(&root.join("Torus SemiBold.otf"), "score list font")
                .map_err(|_| invalid("Torus SemiBold.otf"))?,
            bold: load_font(
                &root.join("ResourceHanRoundedCN-Bold.ttf"),
                "score list bold font",
            )
            .map_err(|_| invalid("ResourceHanRoundedCN-Bold.ttf"))?,
            title: image(&pic, "title-lengthen.png", 450, 100)?,
            design: image(&pic, "design.png", 1_000, 90)?,
            aurora: image(&pic, "aurora.png", 4_096, 1_024)?,
            shines: image(&pic, "bg_shines.png", 2_048, 1_024)?,
            pattern: image(&pic, "pattern.png", 1_400, 512)?,
            rainbow: image(&pic, "rainbow.png", 1_024, 512)?,
            rainbow_bottom: image(&pic, "rainbow_bottom.png", 2_048, 512)?,
            cards: load_names(&pic, &CARD_NAMES, |name| name.to_owned(), 264, 109)?,
            standard: image(&pic, "SD.png", 140, 52)?,
            deluxe: image(&pic, "DX.png", 140, 52)?,
            ranks: load_names(
                &pic,
                &RANK_NAMES,
                |name| format!("UI_TTR_Rank_{name}.png"),
                256,
                120,
            )?,
            combo: load_names(
                &pic,
                &COMBO_NAMES,
                |name| format!("UI_MSS_MBase_Icon_{name}.png"),
                80,
                80,
            )?,
            sync: load_names(
                &pic,
                &SYNC_NAMES,
                |name| format!("UI_MSS_MBase_Icon_{name}.png"),
                80,
                80,
            )?,
            dx_gauges: (1..=5)
                .map(|index| {
                    image(
                        &pic,
                        &format!("UI_GAM_Gauge_DXScoreIcon_0{index}.png"),
                        80,
                        64,
                    )
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

fn require_files(root: &Path, pic: &Path) -> Result<(), ScoreListRenderError> {
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
        "title-lengthen.png",
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
    paths.extend(COMBO_NAMES.map(|name| {
        let file = format!("UI_MSS_MBase_Icon_{name}.png");
        (format!("mai/pic/{file}"), pic.join(file))
    }));
    paths.extend(SYNC_NAMES.map(|name| {
        let file = format!("UI_MSS_MBase_Icon_{name}.png");
        (format!("mai/pic/{file}"), pic.join(file))
    }));
    paths.extend((1..=5).map(|index| {
        let file = format!("UI_GAM_Gauge_DXScoreIcon_0{index}.png");
        (format!("mai/pic/{file}"), pic.join(file))
    }));
    let missing = paths
        .into_iter()
        .filter_map(|(name, path)| (!path.is_file()).then_some(name))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(ScoreListRenderError::AssetsRequired { missing })
    }
}

fn image(
    root: &Path,
    name: &str,
    max_width: u32,
    max_height: u32,
) -> Result<RgbaImage, ScoreListRenderError> {
    decode_limited(
        &root.join(name),
        "score list image",
        max_width,
        max_height,
        32 * 1024 * 1024,
    )
    .map_err(|_| invalid(&format!("mai/pic/{name}")))
}

fn invalid(name: &str) -> ScoreListRenderError {
    ScoreListRenderError::InvalidAsset {
        name: name.to_owned(),
    }
}

fn load_names<const N: usize>(
    root: &Path,
    names: &[&str; N],
    path: impl Fn(&str) -> String,
    max_width: u32,
    max_height: u32,
) -> Result<Vec<RgbaImage>, ScoreListRenderError> {
    names
        .iter()
        .map(|name| image(root, &path(name), max_width, max_height))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::require_files;

    #[test]
    fn missing_asset_report_never_contains_the_configured_root() {
        let root = Path::new("/secret-score-list-root");
        let error = require_files(root, &root.join("mai/pic"))
            .err()
            .map(|error| error.to_string());
        assert!(error.as_deref().is_some_and(|text| {
            text.contains("Torus SemiBold.otf") && !text.contains("secret-score-list-root")
        }));
    }
}
