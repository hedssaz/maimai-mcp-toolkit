use image::{
    Rgba, RgbaImage,
    imageops::{FilterType, overlay, resize},
};
use imageproc::drawing::draw_filled_rect_mut;
use imageproc::rect::Rect;

use super::{assets::CompletionAssets, model::ScoreCardCell};
use crate::{
    CoverResolver, RenderError,
    cover::ResolvedCover,
    text::{draw_text, fit_text},
};

pub(crate) fn draw_cover_id_cell(
    image: &mut RgbaImage,
    origin: (i32, i32),
    cell: &ScoreCardCell,
    assets: &CompletionAssets,
    covers: &CoverResolver,
) -> Result<bool, RenderError> {
    let (cover, placeholder) = cover(cell, covers)?;
    let (x, y) = origin;
    overlay(
        image,
        &resize(&cover, 55, 55, FilterType::Lanczos3),
        i64::from(x),
        i64::from(y),
    );
    draw_filled_rect_mut(
        image,
        Rect::at(x, y + 43).of_size(55, 12),
        Rgba([124, 130, 255, 225]),
    );
    let id = match &cell.cover_id {
        maimai_core::SongIdValue::Numeric(value) => value.to_string(),
        maimai_core::SongIdValue::Text(value) => value.as_str().to_owned(),
    };
    draw_text(
        image,
        (x + 3, y + 42),
        &fit_text(&id, 49, 11.0, &assets.regular),
        11.0,
        Rgba([255, 255, 255, 255]),
        &assets.regular,
    );
    Ok(placeholder)
}

fn cover(cell: &ScoreCardCell, covers: &CoverResolver) -> Result<(RgbaImage, bool), RenderError> {
    match covers.resolve_music(Some(&cell.cover_id), cell.image_name.as_deref())? {
        ResolvedCover::Image(image) => Ok((image, false)),
        ResolvedCover::Missing(_) => Ok((
            RgbaImage::from_pixel(100, 100, Rgba([224, 229, 245, 255])),
            true,
        )),
    }
}
