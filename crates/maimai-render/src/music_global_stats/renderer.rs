use std::{f64::consts::TAU, io::Cursor, path::Path};

use ab_glyph::FontArc;
use image::{ColorType, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};
use imageproc::{
    drawing::{draw_filled_circle_mut, draw_filled_rect_mut, text_size},
    rect::Rect,
};

use super::{
    assets::MusicGlobalStatsAssets,
    model::{MusicGlobalStatsRenderedPng, MusicGlobalStatsView, difficulty_label},
};
use crate::{
    RenderError,
    text::{draw_text, fit_text, middle_origin_y},
};

const WIDTH: u32 = 1_000;
const HEIGHT: u32 = 800;
const INK: Rgba<u8> = Rgba([44, 52, 64, 255]);
const MUTED: Rgba<u8> = Rgba([100, 116, 139, 255]);
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
const EMPTY: Rgba<u8> = Rgba([241, 245, 249, 255]);
const COLORS: [Rgba<u8>; 14] = [
    Rgba([226, 232, 240, 255]),
    Rgba([94, 234, 212, 255]),
    Rgba([96, 165, 250, 255]),
    Rgba([129, 140, 248, 255]),
    Rgba([244, 114, 182, 255]),
    Rgba([251, 146, 60, 255]),
    Rgba([250, 204, 21, 255]),
    Rgba([74, 222, 128, 255]),
    Rgba([45, 212, 191, 255]),
    Rgba([56, 189, 248, 255]),
    Rgba([168, 85, 247, 255]),
    Rgba([236, 72, 153, 255]),
    Rgba([248, 113, 113, 255]),
    Rgba([132, 204, 22, 255]),
];
const FC_LABELS: [&str; 5] = ["Not FC", "FC", "FC+", "AP", "AP+"];
const ACHIEVEMENT_LABELS: [&str; 14] = [
    "D", "C", "B", "BB", "BBB", "A", "AA", "AAA", "S", "S+", "SS", "SS+", "SSS", "SSS+",
];

pub struct MusicGlobalStatsRenderer {
    assets: MusicGlobalStatsAssets,
}

impl MusicGlobalStatsRenderer {
    pub fn new(static_root: impl AsRef<Path>) -> Result<Self, RenderError> {
        Ok(Self {
            assets: MusicGlobalStatsAssets::load(static_root.as_ref())?,
        })
    }

    pub fn render(
        &self,
        view: &MusicGlobalStatsView,
    ) -> Result<MusicGlobalStatsRenderedPng, RenderError> {
        let mut image = RgbaImage::from_pixel(WIDTH, HEIGHT, WHITE);
        self.draw_title(&mut image, view);
        self.draw_chart(
            &mut image,
            "FC 分布",
            &FC_LABELS,
            &view.full_combo_distribution,
            (280, 320),
        );
        self.draw_chart(
            &mut image,
            "达成率分布",
            &ACHIEVEMENT_LABELS,
            &view.achievement_distribution,
            (720, 320),
        );
        encode(image)
    }

    fn draw_title(&self, image: &mut RgbaImage, view: &MusicGlobalStatsView) {
        let id = view
            .display_id
            .map_or_else(String::new, |value| value.to_string());
        let title = [id.as_str(), view.title.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let title = format!("{title} [{}]", difficulty_label(view.difficulty));
        let title = fit_text(&title, 930, 32.0, &self.assets.bold);
        centered_text(image, (500, 42), &title, 32.0, INK, &self.assets.bold);
    }

    fn draw_chart<const N: usize>(
        &self,
        image: &mut RgbaImage,
        title: &str,
        labels: &[&str; N],
        values: &[u64; N],
        center: (i32, i32),
    ) {
        centered_text(
            image,
            (center.0, center.1 - 205),
            title,
            30.0,
            INK,
            &self.assets.bold,
        );
        let total = values.iter().map(|value| u128::from(*value)).sum::<u128>();
        draw_donut(image, center, 160, 58, values);
        if total == 0 {
            centered_text(image, center, "No Data", 24.0, MUTED, &self.assets.regular);
            return;
        }
        centered_text(
            image,
            (center.0, center.1 - 8),
            &total.to_string(),
            24.0,
            INK,
            &self.assets.regular,
        );
        centered_text(
            image,
            (center.0, center.1 + 18),
            "Total",
            16.0,
            MUTED,
            &self.assets.regular,
        );
        self.draw_legend(image, labels, values, total, center);
    }

    fn draw_legend<const N: usize>(
        &self,
        image: &mut RgbaImage,
        labels: &[&str; N],
        values: &[u64; N],
        total: u128,
        center: (i32, i32),
    ) {
        let columns = if N > 7 { 2 } else { 1 };
        for (index, (label, value)) in labels.iter().zip(values).enumerate() {
            let column = index % columns;
            let row = index / columns;
            let column = i32::try_from(column).map_or(0, |value| value);
            let row = i32::try_from(row).map_or(0, |value| value);
            let x = center.0 - 185 + column * 170;
            let y = center.1 + 202 + row * 28;
            draw_filled_rect_mut(
                image,
                Rect::at(x, y - 10).of_size(18, 18),
                COLORS[index % COLORS.len()],
            );
            let percent = if total == 0 {
                0.0
            } else {
                (*value as f64) * 100.0 / total as f64
            };
            draw_text(
                image,
                (x + 26, y - 9),
                &format!("{label} {value} ({percent:.1}%)"),
                16.0,
                INK,
                &self.assets.regular,
            );
        }
    }
}

fn draw_donut<const N: usize>(
    image: &mut RgbaImage,
    center: (i32, i32),
    radius: i32,
    inner_radius: i32,
    values: &[u64; N],
) {
    let total = values.iter().map(|value| u128::from(*value)).sum::<u128>();
    if total == 0 {
        draw_filled_circle_mut(image, center, radius, EMPTY);
        draw_filled_circle_mut(image, center, inner_radius, WHITE);
        return;
    }
    let radius_squared = i64::from(radius) * i64::from(radius);
    let inner_squared = i64::from(inner_radius) * i64::from(inner_radius);
    for y in -radius..=radius {
        for x in -radius..=radius {
            let distance = i64::from(x) * i64::from(x) + i64::from(y) * i64::from(y);
            if distance > radius_squared || distance < inner_squared {
                continue;
            }
            let angle = (f64::from(y).atan2(f64::from(x)) + TAU / 4.0).rem_euclid(TAU);
            let position = angle / TAU * total as f64;
            let mut cumulative = 0_u128;
            let mut color = COLORS[0];
            for (index, value) in values.iter().enumerate() {
                cumulative += u128::from(*value);
                if position < cumulative as f64 {
                    color = COLORS[index % COLORS.len()];
                    break;
                }
            }
            if let (Ok(px), Ok(py)) = (u32::try_from(center.0 + x), u32::try_from(center.1 + y))
                && px < image.width()
                && py < image.height()
            {
                image.put_pixel(px, py, color);
            }
        }
    }
}

fn centered_text(
    image: &mut RgbaImage,
    center: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
) {
    let (width, _) = text_size(size, font, text);
    let width = i32::try_from(width).map_or(i32::MAX, |value| value);
    draw_text(
        image,
        (
            center.0 - width / 2,
            middle_origin_y(center.1, text, size, font),
        ),
        text,
        size,
        color,
        font,
    );
}

fn encode(image: RgbaImage) -> Result<MusicGlobalStatsRenderedPng, RenderError> {
    let mut bytes = Vec::new();
    PngEncoder::new(Cursor::new(&mut bytes))
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ColorType::Rgba8.into(),
        )
        .map_err(RenderError::PngEncode)?;
    Ok(MusicGlobalStatsRenderedPng {
        bytes,
        width: WIDTH,
        height: HEIGHT,
    })
}
