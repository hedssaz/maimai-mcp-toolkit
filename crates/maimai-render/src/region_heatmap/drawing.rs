use ab_glyph::FontArc;
use image::{Rgba, RgbaImage};
use imageproc::{
    drawing::{
        draw_filled_circle_mut, draw_filled_rect_mut, draw_line_segment_mut, draw_polygon_mut,
        draw_text_mut, text_size,
    },
    point::Point,
    rect::Rect,
};

pub(super) fn text_dimensions(text: &str, size: f32, font: &FontArc) -> (i32, i32) {
    let (width, height) = text_size(size, font, text);
    (
        i32::try_from(width).unwrap_or(i32::MAX),
        i32::try_from(height).unwrap_or(i32::MAX),
    )
}

pub(super) fn draw_text(
    image: &mut RgbaImage,
    position: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
) {
    draw_text_mut(image, color, position.0, position.1, size, font, text);
}

pub(super) fn centered_text(
    image: &mut RgbaImage,
    center: (f64, f64),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
    stroke: Option<(Rgba<u8>, i32)>,
) {
    let (width, _) = text_dimensions(text, size, font);
    let position = (
        (center.0 - f64::from(width) / 2.0) as i32,
        crate::text::middle_origin_y(center.1.round() as i32, text, size, font),
    );
    if let Some((stroke_color, radius)) = stroke {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx == 0 && dy == 0 {
                    continue;
                }
                draw_text(
                    image,
                    (position.0 + dx, position.1 + dy),
                    text,
                    size,
                    stroke_color,
                    font,
                );
            }
        }
    }
    draw_text(image, position, text, size, color, font);
}

pub(super) fn filled_rounded_rect(
    image: &mut RgbaImage,
    bounds: RectBox,
    radius: i32,
    color: Rgba<u8>,
) {
    let width = bounds.right.saturating_sub(bounds.left).max(0) as u32;
    let height = bounds.bottom.saturating_sub(bounds.top).max(0) as u32;
    if width == 0 || height == 0 {
        return;
    }
    let radius = radius
        .max(0)
        .min(i32::try_from(width / 2).unwrap_or(i32::MAX))
        .min(i32::try_from(height / 2).unwrap_or(i32::MAX));
    let horizontal_width = width.saturating_sub(u32::try_from(radius * 2).unwrap_or(width));
    let vertical_height = height.saturating_sub(u32::try_from(radius * 2).unwrap_or(height));
    if horizontal_width > 0 {
        draw_filled_rect_mut(
            image,
            Rect::at(bounds.left + radius, bounds.top).of_size(horizontal_width, height),
            color,
        );
    }
    if vertical_height > 0 {
        draw_filled_rect_mut(
            image,
            Rect::at(bounds.left, bounds.top + radius).of_size(width, vertical_height),
            color,
        );
    }
    for center in [
        (bounds.left + radius, bounds.top + radius),
        (bounds.right - radius - 1, bounds.top + radius),
        (bounds.left + radius, bounds.bottom - radius - 1),
        (bounds.right - radius - 1, bounds.bottom - radius - 1),
    ] {
        draw_filled_circle_mut(image, center, radius, color);
    }
}

pub(super) fn outlined_rounded_rect(
    image: &mut RgbaImage,
    bounds: RectBox,
    radius: i32,
    fill: Rgba<u8>,
    outline: Rgba<u8>,
    width: i32,
) {
    filled_rounded_rect(image, bounds, radius, outline);
    filled_rounded_rect(
        image,
        RectBox {
            left: bounds.left + width,
            top: bounds.top + width,
            right: bounds.right - width,
            bottom: bounds.bottom - width,
        },
        (radius - width).max(0),
        fill,
    );
}

pub(super) fn polygon(image: &mut RgbaImage, points: &[(i32, i32)], fill: Rgba<u8>) {
    let mut end = points.len();
    while end > 1 && points.first() == points.get(end - 1) {
        end -= 1;
    }
    let points = &points[..end];
    if points.len() < 3 {
        return;
    }
    let points = points
        .iter()
        .map(|(x, y)| Point::new(*x, *y))
        .collect::<Vec<_>>();
    draw_polygon_mut(image, &points, fill);
}

pub(super) fn line(
    image: &mut RgbaImage,
    from: (f32, f32),
    to: (f32, f32),
    color: Rgba<u8>,
    width: i32,
) {
    let radius = width.saturating_sub(1) / 2;
    for offset in -radius..=radius {
        draw_line_segment_mut(
            image,
            (from.0 + offset as f32, from.1),
            (to.0 + offset as f32, to.1),
            color,
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RectBox {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectBox {
    pub(super) const fn intersects(self, other: Self, padding: i32) -> bool {
        !(self.right + padding <= other.left
            || other.right + padding <= self.left
            || self.bottom + padding <= other.top
            || other.bottom + padding <= self.top)
    }

    pub(super) const fn inside(self, bounds: Self) -> bool {
        self.left >= bounds.left
            && self.top >= bounds.top
            && self.right <= bounds.right
            && self.bottom <= bounds.bottom
    }

    pub(super) fn overlap_area(self, other: Self) -> i32 {
        let width = (self.right.min(other.right) - self.left.max(other.left)).max(0);
        let height = (self.bottom.min(other.bottom) - self.top.max(other.top)).max(0);
        width.saturating_mul(height)
    }
}
