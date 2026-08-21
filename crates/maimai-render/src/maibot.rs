use std::path::Path;

use image::{
    Rgba, RgbaImage,
    imageops::{FilterType, blur, crop_imm, overlay, resize},
};
use imageproc::{
    drawing::{draw_filled_rect_mut, draw_polygon_mut, text_size},
    point::Point,
    rect::Rect,
};

use crate::{
    B50View, CoverResolver, Difficulty, LegacyRenderer, MissingCover, RenderError, RenderedPng,
    ScoreCard, ScoreSection,
    cover::ResolvedCover,
    template::{TemplateAssets, encoded, paste},
    text::{draw_text, fit_text, format_achievement, format_constant},
};

const LEVEL_COLORS: [Rgba<u8>; 5] = [
    Rgba([69, 193, 36, 255]),
    Rgba([255, 186, 1, 255]),
    Rgba([255, 90, 102, 255]),
    Rgba([134, 49, 200, 255]),
    Rgba([217, 197, 233, 255]),
];

pub struct MaibotRenderer {
    assets: TemplateAssets,
}

impl MaibotRenderer {
    pub fn new(
        static_root: impl AsRef<Path>,
        legacy: &LegacyRenderer,
    ) -> Result<Self, RenderError> {
        let (regular, bold) = legacy.fonts();
        Ok(Self {
            assets: TemplateAssets::maibot(static_root.as_ref(), regular, bold)?,
        })
    }

    pub fn render(
        &self,
        view: &B50View,
        covers: &CoverResolver,
    ) -> Result<RenderedPng, RenderError> {
        let mut image = self.assets.pic("UI_TTR_BG_Base_Plus.png")?;
        self.draw_header(&mut image, view)?;
        let mut missing = Vec::new();
        for (index, card) in view.b35().iter().enumerate() {
            let row = index / 7;
            let col = index % 7;
            self.draw_card(
                &mut image,
                card,
                ScoreSection::B35,
                index + 1,
                (6 + 138 * col as i32, 120 + 96 * row as i32),
                false,
                covers,
                &mut missing,
            )?;
        }
        for (index, card) in view.b15().iter().enumerate() {
            let row = index / 3;
            let col = index % 3;
            self.draw_card(
                &mut image,
                card,
                ScoreSection::B15,
                index + 1,
                (992 + 138 * col as i32, 120 + 96 * row as i32),
                true,
                covers,
                &mut missing,
            )?;
        }
        encoded(&image, view.card_count(), missing)
    }

    fn draw_header(&self, image: &mut RgbaImage, view: &B50View) -> Result<(), RenderError> {
        let title = self.assets.pic("UI_CMN_TabTitle_MaimaiTitle_Ver214.png")?;
        paste(
            image,
            &title,
            10,
            10,
            Some((title.width() * 65 / 100, title.height() * 65 / 100)),
        );
        let rating = view.player().rating().unwrap_or(view.breakdown().total);
        let mut rating_base = self
            .assets
            .optional_pic(rating_asset(rating))?
            .unwrap_or(self.assets.pic("UI_CMN_DXRating_S_10.png")?);
        for (index, digit) in format!("{rating:05}")
            .chars()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .enumerate()
        {
            if let Some(digit) = self
                .assets
                .optional_pic(&format!("UI_NUM_Drating_{digit}.png"))?
            {
                let digit = resize(
                    &digit,
                    digit.width() * 60 / 100,
                    digit.height() * 60 / 100,
                    FilterType::Lanczos3,
                );
                overlay(&mut rating_base, &digit, 84 + 15 * index as i64, 9);
            }
        }
        let rating_base = resize(
            &rating_base,
            rating_base.width() * 85 / 100,
            rating_base.height() * 85 / 100,
            FilterType::Lanczos3,
        );
        overlay(image, &rating_base, 240, 8);
        let mut plate = resize(
            &self.assets.pic("UI_TST_PlateMask.png")?,
            285,
            40,
            FilterType::Lanczos3,
        );
        let nickname = fit_text(view.player().nickname(), 210, 28.0, &self.assets.bold)
            .chars()
            .map(|character| character.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        draw_text(
            &mut plate,
            (12, 4),
            &nickname,
            28.0,
            Rgba([0, 0, 0, 255]),
            &self.assets.bold,
        );
        let name_dx = self.assets.pic("UI_CMN_Name_DX.png")?;
        paste(
            &mut plate,
            &name_dx,
            230,
            4,
            Some((name_dx.width() * 90 / 100, name_dx.height() * 90 / 100)),
        );
        paste(image, &plate, 240, 40, None);
        let mut badge = self.assets.pic("UI_CMN_Shougou_Rainbow.png")?;
        let breakdown = view.breakdown();
        let summary = format!("SD: {} + DX: {} = {rating}", breakdown.b35, breakdown.b15);
        let summary = fit_text(
            &summary,
            badge.width().saturating_sub(24),
            14.0,
            &self.assets.bold,
        );
        let (summary_width, _) = text_size(14.0, &self.assets.bold, &summary);
        let summary_x = ((badge.width().saturating_sub(summary_width)) / 2) as i32;
        outlined_text(
            &mut badge,
            (summary_x, 5),
            &summary,
            14.0,
            Rgba([255, 255, 255, 255]),
            &self.assets.bold,
            1,
        );
        let badge = resize(
            &badge,
            badge.width() * 105 / 100,
            badge.height() * 105 / 100,
            FilterType::Lanczos3,
        );
        overlay(image, &badge, 240, 83);
        self.paste_pic(image, "UI_RSL_MBase_Parts_01.png", 988, 65, None)?;
        self.paste_pic(image, "UI_RSL_MBase_Parts_02.png", 865, 65, None)?;
        draw_text(
            image,
            (1_028, 23),
            &fit_text(view.title(), 260, 20.0, &self.assets.bold),
            20.0,
            Rgba([44, 70, 120, 255]),
            &self.assets.bold,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_card(
        &self,
        image: &mut RgbaImage,
        card: &ScoreCard,
        section: ScoreSection,
        display_index: usize,
        xy: (i32, i32),
        compact_title: bool,
        covers: &CoverResolver,
        missing: &mut Vec<MissingCover>,
    ) -> Result<(), RenderError> {
        let (x, y) = xy;
        draw_filled_rect_mut(
            image,
            Rect::at(x + 1, y + 1).of_size(131, 88),
            Rgba([0, 0, 0, 204]),
        );
        match covers.resolve(card)? {
            ResolvedCover::Image(cover) => {
                let scaled_height = ((u64::from(cover.height()) * 131)
                    / u64::from(cover.width().max(1)))
                .max(88) as u32;
                let cover = resize(&cover, 131, scaled_height, FilterType::Lanczos3);
                let top = scaled_height.saturating_sub(88) / 2;
                let mut cover = blur(&crop_imm(&cover, 0, top, 131, 88).to_image(), 3.0);
                for pixel in cover.pixels_mut() {
                    pixel.0[0] = (u16::from(pixel.0[0]) * 72 / 100) as u8;
                    pixel.0[1] = (u16::from(pixel.0[1]) * 72 / 100) as u8;
                    pixel.0[2] = (u16::from(pixel.0[2]) * 72 / 100) as u8;
                }
                overlay(image, &cover, x.into(), y.into());
            }
            ResolvedCover::Missing(reason) => {
                draw_filled_rect_mut(
                    image,
                    Rect::at(x, y).of_size(131, 88),
                    Rgba([50, 57, 72, 255]),
                );
                missing.push(MissingCover {
                    section,
                    index: display_index,
                    song_id: card.song_id().cloned(),
                    title: card.title().to_owned(),
                    reason,
                });
            }
        }
        draw_polygon_mut(
            image,
            &[
                Point::new(x + 131, y),
                Point::new(x + 104, y),
                Point::new(x + 131, y + 27),
            ],
            LEVEL_COLORS[difficulty_index(card.difficulty())],
        );
        let title_size = if compact_title { 14.0 } else { 16.0 };
        draw_text(
            image,
            (x + 8, y + 8),
            &fit_text(card.title(), 112, title_size, &self.assets.bold),
            title_size,
            Rgba([255, 255, 255, 255]),
            &self.assets.bold,
        );
        let achievement = card
            .achievements()
            .map_or_else(|| "未知".to_owned(), format_achievement);
        draw_text(
            image,
            (x + 7, y + 28),
            &achievement,
            12.0,
            Rgba([255, 255, 255, 255]),
            &self.assets.bold,
        );
        self.optional_scaled_icon(image, rank_asset(card.grade()), x + 72, y + 28, 30)?;
        self.optional_scaled_icon(image, combo_asset(card.combo()), x + 103, y + 27, 45)?;
        self.optional_scaled_icon(image, combo_asset(card.sync()), x + 103, y + 53, 45)?;
        let constant = card
            .constant()
            .map_or_else(|| "-".to_owned(), format_constant);
        draw_text(
            image,
            (x + 8, y + 44),
            &fit_text(
                &format!("Base: {constant} -> {}", card.rating()),
                102,
                12.0,
                &self.assets.bold,
            ),
            12.0,
            Rgba([255, 255, 255, 255]),
            &self.assets.bold,
        );
        draw_text(
            image,
            (x + 8, y + 60),
            &format!("#{display_index}"),
            18.0,
            Rgba([255, 255, 255, 255]),
            &self.assets.bold,
        );
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

    fn optional_scaled_icon(
        &self,
        target: &mut RgbaImage,
        name: Option<String>,
        x: i32,
        y: i32,
        percent: u32,
    ) -> Result<(), RenderError> {
        if let Some(name) = name
            && let Some(asset) = self.assets.optional_pic(&name)?
        {
            paste(
                target,
                &asset,
                x.into(),
                y.into(),
                Some((
                    asset.width() * percent / 100,
                    asset.height() * percent / 100,
                )),
            );
        }
        Ok(())
    }
}

fn outlined_text(
    image: &mut RgbaImage,
    origin: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &ab_glyph::FontArc,
    stroke: i32,
) {
    for dy in -stroke..=stroke {
        for dx in -stroke..=stroke {
            if dx != 0 || dy != 0 {
                draw_text(
                    image,
                    (origin.0 + dx, origin.1 + dy),
                    text,
                    size,
                    Rgba([0, 0, 0, 255]),
                    font,
                );
            }
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
        0..=999 => "UI_CMN_DXRating_S_01.png",
        1_000..=1_999 => "UI_CMN_DXRating_S_02.png",
        2_000..=3_999 => "UI_CMN_DXRating_S_03.png",
        4_000..=6_999 => "UI_CMN_DXRating_S_04.png",
        7_000..=9_999 => "UI_CMN_DXRating_S_05.png",
        10_000..=11_999 => "UI_CMN_DXRating_S_06.png",
        12_000..=12_999 => "UI_CMN_DXRating_S_07.png",
        13_000..=14_499 => "UI_CMN_DXRating_S_08.png",
        14_500..=14_999 => "UI_CMN_DXRating_S_09.png",
        _ => "UI_CMN_DXRating_S_10.png",
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
    Some(format!("UI_GAM_Rank_{code}.png"))
}

fn combo_asset(value: Option<&str>) -> Option<String> {
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
    Some(format!("UI_MSS_MBase_Icon_{code}_S.png"))
}
