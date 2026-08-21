use std::{io::Cursor, path::Path};

use image::{ColorType, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};
use imageproc::drawing::text_size;

use super::{
    assets::RatingRankingAssets,
    model::{RatingRankingDocument, RatingRankingRenderedPng},
};
use crate::{
    RenderError,
    text::{draw_text, fit_text},
};

const FONT_SIZE: f32 = 35.0;
const MIN_LINE_HEIGHT: u32 = 29;
const PADDING: u32 = 10;
const LINE_MARGIN: u32 = 4;
const MIN_WIDTH: u32 = 160;
const MAX_WIDTH: u32 = 1_200;
const MAX_HEIGHT: u32 = 4_096;
const MAX_TEXT_WIDTH: u32 = MAX_WIDTH - PADDING * 2;
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);

pub struct RatingRankingRenderer {
    assets: RatingRankingAssets,
}

impl RatingRankingRenderer {
    pub fn new(static_root: impl AsRef<Path>) -> Result<Self, RenderError> {
        Ok(Self {
            assets: RatingRankingAssets::load(static_root.as_ref())?,
        })
    }

    pub fn render(
        &self,
        document: &RatingRankingDocument,
    ) -> Result<RatingRankingRenderedPng, RenderError> {
        let lines = document
            .lines()
            .iter()
            .map(|line| fit_text(line, MAX_TEXT_WIDTH, FONT_SIZE, &self.assets.font))
            .collect::<Vec<_>>();
        let line_height = lines
            .iter()
            .map(|line| text_size(FONT_SIZE, &self.assets.font, line).1)
            .max()
            .map_or(0, |height| height)
            .max(MIN_LINE_HEIGHT);
        let text_width = lines
            .iter()
            .map(|line| text_size(FONT_SIZE, &self.assets.font, line).0)
            .max()
            .map_or(0, |width| width);
        let width = text_width
            .checked_add(PADDING * 2)
            .ok_or_else(|| {
                RenderError::invalid("rating_ranking.width", "calculated width exceeds u32")
            })?
            .clamp(MIN_WIDTH, MAX_WIDTH);
        let line_count = u32::try_from(lines.len())
            .map_err(|_| RenderError::invalid("rating_ranking.lines", "line count exceeds u32"))?;
        let height = line_count
            .checked_mul(line_height)
            .and_then(|value| {
                line_count
                    .checked_sub(1)
                    .and_then(|count| count.checked_mul(LINE_MARGIN))
                    .and_then(|margin| value.checked_add(margin))
            })
            .and_then(|value| value.checked_add(PADDING * 2))
            .filter(|height| *height <= MAX_HEIGHT)
            .ok_or_else(|| {
                RenderError::invalid(
                    "rating_ranking.height",
                    "calculated height exceeds render limit",
                )
            })?;
        let mut image = RgbaImage::from_pixel(width, height, WHITE);
        for (index, line) in lines.iter().enumerate() {
            let index = i32::try_from(index).map_err(|_| {
                RenderError::invalid("rating_ranking.lines", "line index exceeds i32")
            })?;
            let step = line_height
                .checked_add(LINE_MARGIN)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| {
                    RenderError::invalid("rating_ranking.line_height", "line height exceeds i32")
                })?;
            let y = index
                .checked_mul(step)
                .and_then(|value| value.checked_add(10))
                .ok_or_else(|| {
                    RenderError::invalid("rating_ranking.lines", "line position exceeds i32")
                })?;
            draw_text(
                &mut image,
                (10, y),
                line,
                FONT_SIZE,
                BLACK,
                &self.assets.font,
            );
        }
        encode(image)
    }
}

fn encode(image: RgbaImage) -> Result<RatingRankingRenderedPng, RenderError> {
    let width = image.width();
    let height = image.height();
    let mut bytes = Vec::new();
    PngEncoder::new(Cursor::new(&mut bytes))
        .write_image(image.as_raw(), width, height, ColorType::Rgba8.into())
        .map_err(RenderError::PngEncode)?;
    Ok(RatingRankingRenderedPng {
        bytes,
        width,
        height,
    })
}
