use ab_glyph::FontArc;
use image::{Rgba, RgbaImage};
use imageproc::drawing::text_size;

use crate::text::draw_text;

pub(super) const WIDTH: u32 = 1_400;
pub(super) const GROUP_SIZE: usize = 20;
const CARD_HEIGHT: usize = 109;
const COLUMNS: usize = 5;
const GROUP_STEP: i32 = 576;
const CARD_X_STEP: i32 = 276;
const CARD_Y_STEP: i32 = 114;

pub(super) fn group_y(group: usize) -> i32 {
    i32::try_from(group)
        .ok()
        .and_then(|value| value.checked_mul(GROUP_STEP))
        .unwrap_or(0)
}

pub(super) fn card_origin(group_y: i32, index: usize) -> (i32, i32) {
    let row = index / COLUMNS;
    let column = index % COLUMNS;
    (
        16 + i32::try_from(column).unwrap_or(0) * CARD_X_STEP,
        140 + group_y + i32::try_from(row).unwrap_or(0) * CARD_Y_STEP,
    )
}

pub(super) fn canvas_height(item_count: usize) -> u32 {
    let (rows, groups) = if item_count <= GROUP_SIZE {
        (4, 1)
    } else {
        (
            item_count.div_ceil(COLUMNS),
            item_count.div_ceil(GROUP_SIZE),
        )
    };
    let content = rows * CARD_HEIGHT + groups * 140;
    u32::try_from(150_usize.saturating_add(content)).unwrap_or(2_454)
}

pub(crate) fn gradient(height: u32) -> RgbaImage {
    let mut image = RgbaImage::new(WIDTH, height);
    let colors = [
        [124.0, 129.0, 255.0],
        [193.0, 247.0, 225.0],
        [255.0, 255.0, 255.0],
    ];
    for y in 0..height {
        let position = f64::from(y) / f64::from(height.max(1));
        let (left, right, ratio) = if position < 0.4 {
            (colors[0], colors[1], position / 0.4)
        } else {
            (colors[1], colors[2], (position - 0.4) / 0.6)
        };
        let pixel = Rgba([
            mix(left[0], right[0], ratio),
            mix(left[1], right[1], ratio),
            mix(left[2], right[2], ratio),
            255,
        ]);
        for x in 0..WIDTH {
            image.put_pixel(x, y, pixel);
        }
    }
    image
}

pub(super) fn draw_centered(
    image: &mut RgbaImage,
    center_x: i32,
    y: i32,
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
) {
    let width = i32::try_from(text_size(size, font, text).0).unwrap_or(0);
    draw_text(image, (center_x - width / 2, y), text, size, color, font);
}

fn mix(left: f64, right: f64, ratio: f64) -> u8 {
    ((1.0 - ratio) * left + ratio * right).clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::canvas_height;

    #[test]
    fn legacy_page_heights_do_not_create_an_empty_eighty_record_page() {
        assert_eq!(canvas_height(0), 726);
        assert_eq!(canvas_height(20), 726);
        assert_eq!(canvas_height(21), 975);
        assert_eq!(canvas_height(79), 2_454);
        assert_eq!(canvas_height(80), 2_454);
    }
}
