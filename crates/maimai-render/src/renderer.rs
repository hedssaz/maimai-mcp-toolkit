use std::io::Cursor;

use image::{
    ColorType, ImageEncoder, Rgba, RgbaImage,
    codecs::png::PngEncoder,
    imageops::{FilterType, blur, overlay, resize},
};
use imageproc::{
    drawing::{draw_filled_circle_mut, draw_filled_rect_mut},
    rect::Rect,
};

use crate::{
    B50View, CoverResolver, LegacyAssets, MissingCover, MissingCoverReason, RenderError,
    RenderMetadata, RenderedPng, ScoreCard, ScoreSection,
    assets::{LoadedAssets, load_cover},
    cover::ResolvedCover,
    layout::{CARD_HEIGHT, CARD_WIDTH, HEADER_HEIGHT, WIDTH, canvas_height, card_origin},
    text::{draw_text, fit_text, format_achievement, format_constant},
};

pub struct LegacyRenderer {
    assets: LoadedAssets,
}

impl LegacyRenderer {
    pub fn new(assets: LegacyAssets) -> Result<Self, RenderError> {
        Ok(Self {
            assets: assets.load()?,
        })
    }

    pub fn render(&self, view: &B50View) -> Result<RenderedPng, RenderError> {
        self.render_inner(view, None)
    }

    pub fn render_with_covers(
        &self,
        view: &B50View,
        covers: &CoverResolver,
    ) -> Result<RenderedPng, RenderError> {
        self.render_inner(view, Some(covers))
    }

    fn render_inner(
        &self,
        view: &B50View,
        covers: Option<&CoverResolver>,
    ) -> Result<RenderedPng, RenderError> {
        let height = canvas_height(view.card_count());
        let mut image = self.background(height);
        draw_gradient_header(&mut image);
        self.draw_header(&mut image, view);

        let mut missing_covers = Vec::new();
        for (zero_based, card) in view.b35().iter().enumerate() {
            self.draw_card(
                &mut image,
                card,
                ScoreSection::B35,
                zero_based + 1,
                zero_based,
                covers,
                &mut missing_covers,
            )?;
        }
        let offset = view.b35().len();
        for (zero_based, card) in view.b15().iter().enumerate() {
            self.draw_card(
                &mut image,
                card,
                ScoreSection::B15,
                offset + zero_based + 1,
                offset + zero_based,
                covers,
                &mut missing_covers,
            )?;
        }

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
            metadata: RenderMetadata {
                width: image.width(),
                height: image.height(),
                card_count: view.card_count(),
                missing_covers,
            },
            bytes,
        })
    }

    pub(crate) fn fonts(&self) -> (ab_glyph::FontArc, ab_glyph::FontArc) {
        (
            self.assets.regular_font.clone(),
            self.assets.bold_font.clone(),
        )
    }

    fn background(&self, height: u32) -> RgbaImage {
        let Some(background) = self.assets.background.as_ref() else {
            return RgbaImage::from_pixel(WIDTH, height, rgba(246, 248, 252));
        };
        let resized = resize(background, WIDTH, height, FilterType::Lanczos3);
        let blurred = blur(&resized, 10.0);
        let mut result = RgbaImage::new(WIDTH, height);
        for (target, source) in result.pixels_mut().zip(blurred.pixels()) {
            target.0 = [
                blend_channel(source[0], 246),
                blend_channel(source[1], 248),
                blend_channel(source[2], 252),
                255,
            ];
        }
        result
    }

    fn draw_header(&self, image: &mut RgbaImage, view: &B50View) {
        draw_text(
            image,
            (58, 46),
            view.title(),
            60.0,
            rgba(255, 255, 255),
            &self.assets.bold_font,
        );
        draw_text(
            image,
            (60, 112),
            view.player().nickname(),
            52.0,
            rgba(255, 255, 255),
            &self.assets.bold_font,
        );
        let breakdown = view.breakdown();
        let rating = view
            .player()
            .rating()
            .map_or_else(|| "Unknown".to_owned(), |value| value.to_string());
        let summary = format!(
            "Rating {rating}    B35 {}    B15 {}    Total {}",
            breakdown.b35, breakdown.b15, breakdown.total
        );
        draw_text(
            image,
            (60, 168),
            &summary,
            34.0,
            rgba(235, 245, 255),
            &self.assets.regular_font,
        );

        if let Some(plate) = view.player().plate() {
            fill_rounded_rect(image, (1_530, 58, 1_840, 128), 24, rgba(255, 255, 255));
            let plate = fit_text(plate, 254, 34.0, &self.assets.bold_font);
            draw_text(
                image,
                (1_558, 76),
                &plate,
                34.0,
                rgba(48, 110, 186),
                &self.assets.bold_font,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_card(
        &self,
        image: &mut RgbaImage,
        card: &ScoreCard,
        section: ScoreSection,
        display_index: usize,
        layout_index: usize,
        covers: Option<&CoverResolver>,
        missing_covers: &mut Vec<MissingCover>,
    ) -> Result<(), RenderError> {
        let (x, y) = card_origin(layout_index);
        fill_rounded_rect(
            image,
            (x, y, x + CARD_WIDTH as i32, y + CARD_HEIGHT as i32),
            16,
            rgba(218, 225, 236),
        );
        fill_rounded_rect(
            image,
            (
                x + 2,
                y + 2,
                x + CARD_WIDTH as i32 - 2,
                y + CARD_HEIGHT as i32 - 2,
            ),
            14,
            rgba(255, 255, 255),
        );
        let section_color = match section {
            ScoreSection::B35 => rgba(48, 128, 208),
            ScoreSection::B15 => rgba(226, 108, 70),
        };
        fill_rounded_rect(image, (x, y, x + 58, y + 28), 12, section_color);
        draw_text(
            image,
            (x + 11, y + 5),
            &format!("#{display_index}"),
            22.0,
            rgba(255, 255, 255),
            &self.assets.bold_font,
        );

        let cover_box = (x + 16, y + 34, x + 118, y + 136);
        let missing_reason = self.draw_cover(image, card, cover_box, covers)?;
        if let Some(reason) = missing_reason {
            missing_covers.push(MissingCover {
                section,
                index: display_index,
                song_id: card.song_id().cloned(),
                title: card.title().to_owned(),
                reason,
            });
        }

        let text_x = x + 132;
        let max_width = (x + CARD_WIDTH as i32 - text_x - 14).max(1) as u32;
        draw_text(
            image,
            (text_x, y + 34),
            &fit_text(card.title(), max_width, 24.0, &self.assets.bold_font),
            24.0,
            rgba(32, 39, 52),
            &self.assets.bold_font,
        );
        let constant = card.constant().map_or_else(String::new, format_constant);
        let meta = format!(
            "{} / {} / {} / {constant}",
            card.chart_type(),
            card.difficulty(),
            card.level()
        );
        draw_text(
            image,
            (text_x, y + 64),
            &fit_text(&meta, max_width, 19.0, &self.assets.regular_font),
            19.0,
            rgba(93, 103, 118),
            &self.assets.regular_font,
        );
        let achievement = card
            .achievements()
            .map_or_else(|| "Unknown achievement".to_owned(), format_achievement);
        draw_text(
            image,
            (text_x, y + 91),
            &achievement,
            27.0,
            rgba(33, 129, 91),
            &self.assets.bold_font,
        );
        let mut bottom = format!("ra {}", card.rating());
        if let Some(grade) = card.grade() {
            bottom.push_str(&format!("   {}", grade.to_uppercase()));
        }
        let markers = [card.combo(), card.sync()]
            .into_iter()
            .flatten()
            .map(str::to_uppercase)
            .collect::<Vec<_>>()
            .join(" / ");
        if !markers.is_empty() {
            bottom.push_str("   ");
            bottom.push_str(&markers);
        }
        draw_text(
            image,
            (text_x, y + 123),
            &fit_text(&bottom, max_width, 19.0, &self.assets.bold_font),
            19.0,
            rgba(73, 84, 102),
            &self.assets.bold_font,
        );
        Ok(())
    }

    fn draw_cover(
        &self,
        image: &mut RgbaImage,
        card: &ScoreCard,
        box_: (i32, i32, i32, i32),
        covers: Option<&CoverResolver>,
    ) -> Result<Option<MissingCoverReason>, RenderError> {
        let resolved = match covers {
            Some(covers) => covers.resolve(card)?,
            None => match card.cover_path() {
                Some(path) => match load_cover(path)? {
                    Some(cover) => ResolvedCover::Image(cover),
                    None => ResolvedCover::Missing(MissingCoverReason::Unreadable),
                },
                None => ResolvedCover::Missing(MissingCoverReason::NotProvided),
            },
        };
        let reason = match resolved {
            ResolvedCover::Image(cover) => {
                let cover = resize(&cover, 102, 102, FilterType::Lanczos3);
                overlay(image, &cover, box_.0.into(), box_.1.into());
                return Ok(None);
            }
            ResolvedCover::Missing(reason) => reason,
        };
        fill_rounded_rect(image, box_, 8, rgba(194, 202, 214));
        fill_rounded_rect(
            image,
            (box_.0 + 1, box_.1 + 1, box_.2 - 1, box_.3 - 1),
            7,
            rgba(226, 231, 239),
        );
        let title = fit_text(card.title(), 76, 17.0, &self.assets.regular_font);
        draw_text(
            image,
            (box_.0 + 13, box_.1 + 36),
            &title,
            17.0,
            rgba(96, 106, 122),
            &self.assets.regular_font,
        );
        Ok(Some(reason))
    }
}

fn draw_gradient_header(image: &mut RgbaImage) {
    for y in 0..HEADER_HEIGHT {
        let ratio = y as f32 / (HEADER_HEIGHT - 1) as f32;
        let color = Rgba([
            (34.0 + 36.0 * ratio) as u8,
            (110.0 + 50.0 * ratio) as u8,
            (180.0 + 45.0 * ratio) as u8,
            255,
        ]);
        for x in 0..WIDTH {
            image.put_pixel(x, y, color);
        }
    }
}

pub(crate) fn fill_rounded_rect(
    image: &mut RgbaImage,
    box_: (i32, i32, i32, i32),
    radius: i32,
    color: Rgba<u8>,
) {
    let (x0, y0, x1, y1) = box_;
    let width = (x1 - x0).max(0) as u32;
    let height = (y1 - y0).max(0) as u32;
    if width == 0 || height == 0 {
        return;
    }
    let radius = radius.min((width / 2) as i32).min((height / 2) as i32);
    draw_filled_rect_mut(
        image,
        Rect::at(x0 + radius, y0).of_size(width - 2 * radius as u32, height),
        color,
    );
    draw_filled_rect_mut(
        image,
        Rect::at(x0, y0 + radius).of_size(width, height - 2 * radius as u32),
        color,
    );
    for center in [
        (x0 + radius, y0 + radius),
        (x1 - radius - 1, y0 + radius),
        (x0 + radius, y1 - radius - 1),
        (x1 - radius - 1, y1 - radius - 1),
    ] {
        draw_filled_circle_mut(image, center, radius, color);
    }
}

fn rgba(red: u8, green: u8, blue: u8) -> Rgba<u8> {
    Rgba([red, green, blue, 255])
}

fn blend_channel(background: u8, overlay: u8) -> u8 {
    (f32::from(background) * 0.28 + f32::from(overlay) * 0.72).round() as u8
}
