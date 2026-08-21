use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_text_mut, text_size};
use maimai_core::{AchievementRate, ChartConstant, PlayAchievement};

pub(crate) fn fit_text(text: &str, max_width: u32, size: f32, font: &FontArc) -> String {
    if text_size(size, font, text).0 <= max_width {
        return text.to_owned();
    }
    let ellipsis = "...";
    let characters = text.chars().collect::<Vec<_>>();
    let mut lower = 0;
    let mut upper = characters.len();
    while lower < upper {
        let middle = lower + (upper - lower).div_ceil(2);
        let current = characters[..middle].iter().collect::<String>();
        let candidate = format!("{current}{ellipsis}");
        if text_size(size, font, &candidate).0 <= max_width {
            lower = middle;
        } else {
            upper = middle - 1;
        }
    }
    let current = characters[..lower].iter().collect::<String>();
    format!("{current}{ellipsis}")
}

pub(crate) fn draw_text(
    image: &mut RgbaImage,
    xy: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
) {
    draw_text_mut(image, color, xy.0, xy.1, size, font, text);
}

/// Returns the draw origin whose rendered ink is vertically centered on `center_y`.
///
/// `imageproc::text_size` reports glyph height but drops the glyph's top bearing.
/// Subtracting half of that height therefore places text visibly below Pillow's
/// `lm` / `mm` anchors. Use the actual outlined-pixel bounds instead.
pub(crate) fn middle_origin_y(center_y: i32, text: &str, size: f32, font: &FontArc) -> i32 {
    let Some((top, bottom)) = ink_vertical_bounds(text, size, font) else {
        return center_y;
    };
    // Pillow's middle anchors leave the visible ink about one pixel below the
    // mathematical center with the bundled fonts. Preserve that optical offset
    // so the Rust output overlays the former Pillow renderer.
    center_y + 1 - (top + bottom) / 2
}

fn ink_vertical_bounds(text: &str, size: f32, font: &FontArc) -> Option<(i32, i32)> {
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let baseline = scaled.ascent();
    let mut top = i32::MAX;
    let mut bottom = i32::MIN;

    for character in text.chars() {
        let glyph = scaled
            .glyph_id(character)
            .with_scale_and_position(scale, point(0.0, baseline));
        let Some(outlined) = font.outline_glyph(glyph) else {
            continue;
        };
        let bounds = outlined.px_bounds();
        let glyph_top = bounds.min.y.round() as i32;
        let glyph_bottom = glyph_top.saturating_add(bounds.height().ceil() as i32);
        top = top.min(glyph_top);
        bottom = bottom.max(glyph_bottom);
    }

    (top <= bottom).then_some((top, bottom))
}

pub(crate) fn format_constant(value: ChartConstant) -> String {
    let mut formatted = value.value().normalize().to_string();
    if !formatted.contains('.') {
        formatted.push_str(".0");
    }
    formatted
}

pub(crate) fn format_achievement(value: AchievementRate) -> String {
    let scaled = value.ten_thousandths();
    format!("{}.{:04}%", scaled / 10_000, scaled % 10_000)
}

pub(crate) fn format_play_achievement(value: PlayAchievement) -> String {
    format!("{}%", value.decimal_string())
}

#[cfg(test)]
mod tests {
    use ab_glyph::FontArc;
    use image::{Rgba, RgbaImage};
    use maimai_core::{ChartConstant, PlayAchievement, UtageScore};

    use super::{draw_text, format_constant, format_play_achievement, middle_origin_y};

    #[test]
    fn constant_keeps_one_decimal_for_integer_values() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            format_constant(ChartConstant::from_decimal_str("13.0")?),
            "13.0"
        );
        assert_eq!(
            format_constant(ChartConstant::from_decimal_str("13.4")?),
            "13.4"
        );
        Ok(())
    }

    #[test]
    fn utage_achievement_is_formatted_without_float_rounding() {
        assert_eq!(
            format_play_achievement(PlayAchievement::from(UtageScore::from_ten_thousandths(
                1_535_756
            ))),
            "153.5756%"
        );
    }

    #[test]
    fn middle_origin_centers_the_rendered_ink() -> Result<(), Box<dyn std::error::Error>> {
        let font =
            FontArc::try_from_slice(include_bytes!("../tests/fixtures/DejaVuSans-ASCII.ttf"))?;
        let mut image = RgbaImage::new(200, 100);
        let center_y = 50;
        draw_text(
            &mut image,
            (10, middle_origin_y(center_y, "Hg", 36.0, &font)),
            "Hg",
            36.0,
            Rgba([255, 255, 255, 255]),
            &font,
        );
        let rows = image
            .enumerate_pixels()
            .filter_map(|(_, y, pixel)| (pixel.0[3] > 0).then_some(y as i32))
            .collect::<Vec<_>>();
        let top = rows.iter().copied().min().ok_or("text has no pixels")?;
        let bottom = rows.iter().copied().max().ok_or("text has no pixels")?;
        assert!((top + bottom - (center_y * 2 + 2)).abs() <= 1);
        Ok(())
    }
}
