use ab_glyph::FontArc;
use image::{Rgba, RgbaImage};
use imageproc::drawing::text_size;

use crate::text::{draw_text, middle_origin_y};

pub(super) const SOURCE_WIDTH: u32 = 1_400;
pub(super) const OUTPUT_WIDTH: u32 = 1_000;
pub(super) const CROP_LEFT: u32 = 200;

pub(super) fn card_origin(section_current: bool, index: usize) -> (i32, i32) {
    let x = if section_current { 700 } else { 200 };
    let offset = i32::try_from(index).unwrap_or(0).saturating_mul(140);
    (x, 120_i32.saturating_add(offset))
}

pub(super) fn gradient(height: u32) -> RgbaImage {
    let mut image = RgbaImage::new(SOURCE_WIDTH, height);
    let colors = [[124_u8, 129, 255], [193_u8, 247, 225], [255_u8, 255, 255]];
    let first_end = u64::from(height).saturating_mul(4) / 10;
    for y in 0..height {
        let y = u64::from(y);
        let pixel = if y <= first_end {
            interpolate(colors[0], colors[1], y, first_end.max(1))
        } else {
            interpolate(
                colors[1],
                colors[2],
                y - first_end,
                u64::from(height).saturating_sub(first_end).max(1),
            )
        };
        for x in 0..SOURCE_WIDTH {
            image.put_pixel(x, u32::try_from(y).unwrap_or(0), pixel);
        }
    }
    image
}

pub(super) fn draw_centered(
    image: &mut RgbaImage,
    center_x: i32,
    center_y: i32,
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
) {
    let width = i32::try_from(text_size(size, font, text).0).unwrap_or(0);
    draw_text(
        image,
        (
            center_x - width / 2,
            middle_origin_y(center_y, text, size, font),
        ),
        text,
        size,
        color,
        font,
    );
}

fn interpolate(left: [u8; 3], right: [u8; 3], numerator: u64, denominator: u64) -> Rgba<u8> {
    Rgba([
        mix(left[0], right[0], numerator, denominator),
        mix(left[1], right[1], numerator, denominator),
        mix(left[2], right[2], numerator, denominator),
        255,
    ])
}

fn mix(left: u8, right: u8, numerator: u64, denominator: u64) -> u8 {
    let numerator = numerator.min(denominator);
    let left_weight = denominator - numerator;
    let value = u64::from(left) * left_weight + u64::from(right) * numerator;
    u8::try_from(value / denominator).unwrap_or(right)
}
