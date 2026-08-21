use std::{error::Error, path::PathBuf};

use image::RgbaImage;
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, SongIdNamespace,
    SongIdValue, SourceSongId,
};
use maimai_render::{RiseScoreCandidate, RiseScoreRenderer, RiseScoreView};
use tempfile::TempDir;

#[test]
fn rise_score_text_matches_the_legacy_visual_scale() -> Result<(), Box<dyn Error>> {
    let cache = TempDir::new()?;
    let renderer = RiseScoreRenderer::new(static_root(), cache.path())?;
    let view = RiseScoreView {
        legacy: (0..3)
            .map(|index| candidate(index, ChartGeneration::Standard))
            .collect::<Result<_, _>>()?,
        legacy_replacement_floor: 250,
        current: (3..6)
            .map(|index| candidate(index, ChartGeneration::Deluxe))
            .collect::<Result<_, _>>()?,
        current_replacement_floor: 250,
    };

    let rendered = renderer.render(&view)?;
    assert_eq!((rendered.width, rendered.height), (1_000, 680));
    assert_eq!(rendered.placeholder_covers, 0);
    let image = image::load_from_memory(&rendered.bytes)?.to_rgba8();
    let left_header =
        ink_bounds(&image, 100, 400, 25, 105).ok_or("legacy rise-score header was not rendered")?;
    let footer =
        ink_bounds(&image, 150, 850, 570, 640).ok_or("rise-score credit was not rendered")?;
    assert_eq!((left_header.2, left_header.3), (122, 16));
    assert_eq!((footer.2, footer.3), (525, 19));
    Ok(())
}

fn candidate(
    index: u32,
    generation: ChartGeneration,
) -> Result<RiseScoreCandidate, Box<dyn Error>> {
    Ok(RiseScoreCandidate {
        key: ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::DivingFish, 8),
            generation,
            Difficulty::Master,
        )?,
        display_id: SongIdValue::Numeric(8),
        cover_id: SongIdValue::Numeric(8),
        image_name: None,
        title: format!("相信彩虹 Visual Review {index}"),
        constant: ChartConstant::from_decimal_str("13.7")?,
        old_achievement: Some(AchievementRate::from_decimal_str("99.5")?),
        old_rating: 260,
        target_achievement: AchievementRate::from_decimal_str("100.5")?,
        target_rating: 280,
        gain: 20,
    })
}

fn static_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../maimaidx_render_mcp/static")
}

fn ink_bounds(
    image: &RgbaImage,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
) -> Option<(u32, u32, u32, u32)> {
    let points = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|(x, y)| image.get_pixel(*x, *y).0 == [124, 129, 255, 255])
        .collect::<Vec<_>>();
    let min_x = points.iter().map(|(x, _)| *x).min()?;
    let max_x = points.iter().map(|(x, _)| *x).max()?;
    let min_y = points.iter().map(|(_, y)| *y).min()?;
    let max_y = points.iter().map(|(_, y)| *y).max()?;
    Some((min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
}
