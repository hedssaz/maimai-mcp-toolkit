use std::path::Path;

use image::{
    Rgba, RgbaImage,
    imageops::{FilterType, overlay, resize},
};
use imageproc::drawing::text_size;

use crate::{
    B50View, CoverResolver, Difficulty, MissingCover, RenderError, RenderedPng, ScoreCard,
    ScoreSection,
    cover::ResolvedCover,
    template::{TemplateAssets, encoded, paste},
    text::{draw_text, fit_text, format_achievement, format_constant, middle_origin_y},
};

const DIFFICULTY_ASSETS: [&str; 5] = [
    "b50_score_basic.png",
    "b50_score_advanced.png",
    "b50_score_expert.png",
    "b50_score_master.png",
    "b50_score_remaster.png",
];
const ID_COLORS: [Rgba<u8>; 5] = [
    Rgba([129, 217, 85, 255]),
    Rgba([245, 189, 21, 255]),
    Rgba([255, 129, 141, 255]),
    Rgba([159, 81, 220, 255]),
    Rgba([138, 0, 226, 255]),
];
const CARD_TEXT_COLORS: [Rgba<u8>; 5] = [
    Rgba([255, 255, 255, 255]),
    Rgba([255, 255, 255, 255]),
    Rgba([255, 255, 255, 255]),
    Rgba([255, 255, 255, 255]),
    Rgba([138, 0, 226, 255]),
];

pub struct YuzuRenderer {
    assets: TemplateAssets,
}

impl YuzuRenderer {
    pub fn new(static_root: impl AsRef<Path>) -> Result<Self, RenderError> {
        Ok(Self {
            assets: TemplateAssets::yuzu(static_root.as_ref())?,
        })
    }

    pub fn render(
        &self,
        view: &B50View,
        covers: &CoverResolver,
    ) -> Result<RenderedPng, RenderError> {
        let mut image = self.assets.pic("b50_bg.png")?;
        self.draw_header(&mut image, view)?;
        let mut missing = Vec::new();
        self.draw_section(
            &mut image,
            view.b35(),
            ScoreSection::B35,
            235,
            0,
            covers,
            &mut missing,
        )?;
        self.draw_section(
            &mut image,
            view.b15(),
            ScoreSection::B15,
            1_085,
            view.b35().len(),
            covers,
            &mut missing,
        )?;
        encoded(&image, view.card_count(), missing)
    }

    fn draw_header(&self, image: &mut RgbaImage, view: &B50View) -> Result<(), RenderError> {
        self.paste_pic(image, "logo.png", 14, 60, Some((249, 120)))?;
        let plate = view.player().plate().and_then(|value| safe_plate(value));
        let plate = plate
            .map(|value| {
                self.assets
                    .root
                    .join("mai/plate")
                    .join(format!("{value}.png"))
            })
            .filter(|path| path.is_file());
        let plate = match plate {
            Some(path) => {
                crate::assets::decode_limited(&path, "yuzu plate", 4_096, 4_096, 128 * 1024 * 1024)?
            }
            None => self.assets.pic("UI_Plate_300501.png")?,
        };
        paste(image, &plate, 300, 60, Some((800, 130)));
        self.paste_pic(image, "UI_Icon_309503.png", 305, 65, Some((120, 120)))?;
        let rating = view.player().rating().unwrap_or(view.breakdown().total);
        let rating_asset = rating_asset(rating);
        let rating_base = self
            .assets
            .optional_pic(rating_asset)?
            .unwrap_or(self.assets.pic("UI_CMN_DXRating_10.png")?);
        paste(image, &rating_base, 435, 72, Some((186, 35)));
        for (index, digit) in format!("{rating:05}")
            .chars()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .enumerate()
        {
            if let Some(asset) = self
                .assets
                .optional_pic(&format!("UI_NUM_Drating_{digit}.png"))?
            {
                paste(image, &asset, 520 + 15 * index as i64, 80, Some((17, 20)));
            }
        }
        self.paste_pic(image, "Name.png", 435, 115, None)?;
        self.paste_pic(image, "UI_DNM_DaniPlate_00.png", 625, 120, Some((80, 32)))?;
        self.paste_pic(image, "UI_FBR_Class_00.png", 620, 60, Some((90, 54)))?;
        self.paste_pic(
            image,
            "UI_CMN_Shougou_Rainbow.png",
            435,
            160,
            Some((270, 27)),
        )?;
        let nickname = fit_text(view.player().nickname(), 220, 30.0, &self.assets.bold);
        draw_middle_left_text(
            image,
            (445, 135),
            &nickname,
            30.0,
            Rgba([0, 0, 0, 255]),
            &self.assets.bold,
        );
        let breakdown = view.breakdown();
        draw_centered_stroked_text(
            image,
            (570, 172),
            &format!("B35: {} + B15: {} = {rating}", breakdown.b35, breakdown.b15),
            20.0,
            Rgba([0, 0, 0, 255]),
            &self.assets.regular,
            2,
            Rgba([255, 255, 255, 255]),
        );
        let footer = fit_text(view.title(), 1_340, 32.0, &self.assets.bold);
        draw_centered_stroked_text(
            image,
            (700, 1_570),
            &footer,
            32.0,
            Rgba([124, 129, 255, 255]),
            &self.assets.bold,
            3,
            Rgba([255, 255, 255, 255]),
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_section(
        &self,
        image: &mut RgbaImage,
        cards: &[ScoreCard],
        section: ScoreSection,
        start_y: i32,
        display_offset: usize,
        covers: &CoverResolver,
        missing: &mut Vec<MissingCover>,
    ) -> Result<(), RenderError> {
        for (index, card) in cards.iter().enumerate() {
            let x = 16 + (index % 5) as i32 * 276;
            let y = start_y + (index / 5) as i32 * 114;
            let difficulty = difficulty_index(card.difficulty());
            self.paste_pic(
                image,
                DIFFICULTY_ASSETS[difficulty],
                x.into(),
                y.into(),
                None,
            )?;
            match covers.resolve(card)? {
                ResolvedCover::Image(cover) => {
                    overlay(
                        image,
                        &resize(&cover, 75, 75, FilterType::Lanczos3),
                        (x + 12).into(),
                        (y + 12).into(),
                    );
                }
                ResolvedCover::Missing(reason) => {
                    overlay(
                        image,
                        &RgbaImage::from_pixel(75, 75, Rgba([56, 62, 76, 255])),
                        (x + 12).into(),
                        (y + 12).into(),
                    );
                    missing.push(MissingCover {
                        section,
                        index: display_offset + index + 1,
                        song_id: card.song_id().cloned(),
                        title: card.title().to_owned(),
                        reason,
                    });
                }
            }
            let type_asset = if card.chart_type() == crate::ChartType::Deluxe {
                "DX.png"
            } else {
                "SD.png"
            };
            self.paste_pic(
                image,
                type_asset,
                (x + 51).into(),
                (y + 91).into(),
                Some((37, 14)),
            )?;
            self.optional_icon(image, rank_asset(card.grade()), x + 92, y + 78, (63, 28))?;
            self.optional_icon(
                image,
                combo_asset(card.combo(), false),
                x + 154,
                y + 77,
                (34, 34),
            )?;
            self.optional_icon(
                image,
                combo_asset(card.sync(), false),
                x + 185,
                y + 77,
                (34, 34),
            )?;
            draw_centered_text(
                image,
                (x + 26, y + 98),
                &song_id(card),
                16.5,
                ID_COLORS[difficulty],
                &self.assets.regular,
            );
            draw_middle_left_text(
                image,
                (x + 93, y + 14),
                &fit_text(card.title(), 145, 20.0, &self.assets.bold),
                20.0,
                CARD_TEXT_COLORS[difficulty],
                &self.assets.bold,
            );
            let achievement = card
                .achievements()
                .map_or_else(|| "未知".to_owned(), format_achievement);
            draw_middle_left_text(
                image,
                (x + 93, y + 38),
                &achievement,
                36.0,
                CARD_TEXT_COLORS[difficulty],
                &self.assets.regular,
            );
            let constant = card
                .constant()
                .map_or_else(|| "-".to_owned(), format_constant);
            draw_middle_left_text(
                image,
                (x + 93, y + 65),
                &format!("{constant} -> {}", card.rating()),
                18.0,
                CARD_TEXT_COLORS[difficulty],
                &self.assets.regular,
            );
        }
        Ok(())
    }

    fn paste_pic(
        &self,
        target: &mut RgbaImage,
        name: &str,
        x: i64,
        y: i64,
        size: Option<(u32, u32)>,
    ) -> Result<(), RenderError> {
        paste(target, &self.assets.pic(name)?, x, y, size);
        Ok(())
    }

    fn optional_icon(
        &self,
        target: &mut RgbaImage,
        name: Option<String>,
        x: i32,
        y: i32,
        size: (u32, u32),
    ) -> Result<(), RenderError> {
        if let Some(name) = name
            && let Some(asset) = self.assets.optional_pic(&name)?
        {
            paste(target, &asset, x.into(), y.into(), Some(size));
        }
        Ok(())
    }
}

fn draw_middle_left_text(
    image: &mut RgbaImage,
    position: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &ab_glyph::FontArc,
) {
    draw_text(
        image,
        (position.0, middle_origin_y(position.1, text, size, font)),
        text,
        size,
        color,
        font,
    );
}

fn draw_centered_text(
    image: &mut RgbaImage,
    center: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &ab_glyph::FontArc,
) {
    let (width, _) = text_size(size, font, text);
    draw_text(
        image,
        (
            center.0 - i32::try_from(width).unwrap_or(i32::MAX) / 2,
            middle_origin_y(center.1, text, size, font),
        ),
        text,
        size,
        color,
        font,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_centered_stroked_text(
    image: &mut RgbaImage,
    center: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &ab_glyph::FontArc,
    stroke: i32,
    stroke_color: Rgba<u8>,
) {
    let (width, _) = text_size(size, font, text);
    let origin = (
        center.0 - i32::try_from(width).unwrap_or(i32::MAX) / 2,
        middle_origin_y(center.1, text, size, font),
    );
    for offset_y in -stroke..=stroke {
        for offset_x in -stroke..=stroke {
            if offset_x == 0 && offset_y == 0 {
                continue;
            }
            draw_text(
                image,
                (origin.0 + offset_x, origin.1 + offset_y),
                text,
                size,
                stroke_color,
                font,
            );
        }
    }
    draw_text(image, origin, text, size, color, font);
}

fn difficulty_index(value: Difficulty) -> usize {
    match value {
        Difficulty::Basic => 0,
        Difficulty::Advanced => 1,
        Difficulty::Expert => 2,
        Difficulty::Master => 3,
        Difficulty::ReMaster => 4,
    }
}

fn rating_asset(rating: u32) -> &'static str {
    match rating {
        0..=999 => "UI_CMN_DXRating_01.png",
        1_000..=1_999 => "UI_CMN_DXRating_02.png",
        2_000..=3_999 => "UI_CMN_DXRating_03.png",
        4_000..=6_999 => "UI_CMN_DXRating_04.png",
        7_000..=9_999 => "UI_CMN_DXRating_05.png",
        10_000..=11_999 => "UI_CMN_DXRating_06.png",
        12_000..=12_999 => "UI_CMN_DXRating_07.png",
        13_000..=13_999 => "UI_CMN_DXRating_08.png",
        14_000..=14_499 => "UI_CMN_DXRating_09.png",
        14_500..=14_999 => "UI_CMN_DXRating_10.png",
        _ => "UI_CMN_DXRating_11.png",
    }
}

fn rank_asset(value: Option<&str>) -> Option<String> {
    let code = match value?.to_ascii_lowercase().as_str() {
        "d" => "D",
        "c" => "C",
        "b" => "B",
        "bb" => "BB",
        "bbb" => "BBB",
        "a" => "A",
        "aa" => "AA",
        "aaa" => "AAA",
        "s" => "S",
        "sp" => "Sp",
        "ss" => "SS",
        "ssp" => "SSp",
        "sss" => "SSS",
        "sssp" => "SSSp",
        _ => return None,
    };
    Some(format!("UI_TTR_Rank_{code}.png"))
}

fn combo_asset(value: Option<&str>, small: bool) -> Option<String> {
    let code = match value?.to_ascii_lowercase().as_str() {
        "fc" => "FC",
        "fcp" => "FCp",
        "ap" => "AP",
        "app" => "APp",
        "sync" | "fs" => "FS",
        "fsp" => "FSp",
        "fsd" => "FSD",
        "fsdp" => "FSDp",
        _ => return None,
    };
    Some(format!(
        "UI_MSS_MBase_Icon_{code}{}.png",
        if small { "_S" } else { "" }
    ))
}

fn song_id(card: &ScoreCard) -> String {
    card.song_id()
        .map_or_else(String::new, |id| match id.value() {
            maimai_core::SongIdValue::Numeric(value) => value.to_string(),
            maimai_core::SongIdValue::Text(value) => value.as_str().to_owned(),
        })
}

fn safe_plate(value: &str) -> Option<&str> {
    let path = Path::new(value);
    (path.file_name().and_then(|name| name.to_str()) == Some(value)).then_some(value)
}
