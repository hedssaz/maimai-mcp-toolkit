use std::io::Cursor;

use ab_glyph::FontArc;
use image::{ColorType, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};
use imageproc::drawing::text_size;

use super::model::{MusicInfoChart, MusicInfoRenderedPng};
use crate::{
    RenderError,
    text::{draw_text, format_constant, middle_origin_y},
};

pub(super) fn encode(
    image: RgbaImage,
    used_placeholder_cover: bool,
) -> Result<MusicInfoRenderedPng, RenderError> {
    let mut bytes = Vec::new();
    PngEncoder::new(Cursor::new(&mut bytes))
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ColorType::Rgba8.into(),
        )
        .map_err(RenderError::PngEncode)?;
    Ok(MusicInfoRenderedPng {
        bytes,
        width: image.width(),
        height: image.height(),
        used_placeholder_cover,
    })
}

pub(super) fn level_constant(chart: &MusicInfoChart) -> String {
    match (&*chart.level, chart.constant) {
        ("", None) => String::new(),
        (level, None) => level.to_owned(),
        ("", Some(constant)) => format_constant(constant),
        (level, Some(constant)) => format!("{level}({})", format_constant(constant)),
    }
}

pub(super) fn genre_name(value: &str) -> &str {
    match value {
        "anime" => "POPSアニメ",
        "niconico" => "niconicoボーカロイド",
        "touhou" => "東方Project",
        "game" => "ゲームバラエティ",
        "ongeki" => "オンゲキCHUNITHM",
        "宴会场" => "宴会場",
        other => other,
    }
}

pub(super) fn render_version(value: &str) -> &str {
    match value {
        "GreeN" => "maimai GreeN",
        "GreeN PLUS" => "maimai GreeN PLUS",
        "ORANGE" => "maimai ORANGE",
        "ORANGE PLUS" => "maimai ORANGE PLUS",
        "PiNK" => "maimai PiNK",
        "PiNK PLUS" => "maimai PiNK PLUS",
        "MURASAKi" => "maimai MURASAKi",
        "MURASAKi PLUS" => "maimai MURASAKi PLUS",
        "FiNALE" => "maimai FiNALE",
        "maimaiでらっくす" => "maimai でらっくす",
        "maimaiでらっくす PLUS" => "maimai でらっくす PLUS",
        "Splash" => "maimai でらっくす Splash",
        "Splash PLUS" => "maimai でらっくす Splash PLUS",
        "UNiVERSE" => "maimai でらっくす UNiVERSE",
        "UNiVERSE PLUS" => "maimai でらっくす UNiVERSE PLUS",
        "FESTiVAL" => "maimai でらっくす FESTiVAL",
        "FESTiVAL PLUS" => "maimai でらっくす FESTiVAL PLUS",
        "BUDDiES" => "maimai でらっくす BUDDiES",
        "BUDDiES PLUS" => "maimai でらっくす BUDDiES PLUS",
        "PRiSM" | "PRiSM PLUS" => "maimai でらっくす PRiSM",
        other => other,
    }
}

pub(super) fn rounded_left(image: &RgbaImage, radius: u32) -> RgbaImage {
    let mut result = image.clone();
    let height = result.height();
    for y in 0..height {
        for x in 0..radius {
            let center_y = if y < radius {
                radius
            } else if y >= height.saturating_sub(radius) {
                height.saturating_sub(radius + 1)
            } else {
                continue;
            };
            let dx = radius.saturating_sub(x);
            let dy = y.abs_diff(center_y);
            if dx.saturating_mul(dx) + dy.saturating_mul(dy) > radius.saturating_mul(radius) {
                result.get_pixel_mut(x, y).0[3] = 0;
            }
        }
    }
    result
}

pub(super) fn center_text(
    image: &mut RgbaImage,
    center: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
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

pub(super) fn middle_left_text(
    image: &mut RgbaImage,
    position: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
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
