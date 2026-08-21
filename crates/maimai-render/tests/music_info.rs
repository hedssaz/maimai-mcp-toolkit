use std::{collections::BTreeMap, error::Error, path::PathBuf};

use maimai_core::{
    ChartConstant, ChartGeneration, Difficulty, NoteCounts, SongIdNamespace, SourceSongId,
};
use maimai_render::{MusicInfoChart, MusicInfoRenderer, MusicInfoView, PlayerSongScoreContext};
use tempfile::TempDir;

#[test]
fn music_info_png_matches_static_and_player_context_goldens() -> Result<(), Box<dyn Error>> {
    let cache = TempDir::new()?;
    let renderer = MusicInfoRenderer::new(static_root(), cache.path())?;
    let static_png = renderer.render(&view(None, ChartGeneration::Deluxe)?)?;
    let context = PlayerSongScoreContext::new(
        Some(15_000),
        35,
        20,
        None,
        BTreeMap::from([(Difficulty::Master, 280)]),
    )?;
    let player_png = renderer.render(&view(Some(context), ChartGeneration::Deluxe)?)?;

    assert_eq!((static_png.width, static_png.height), (1_200, 1_300));
    assert_eq!(fnv64(&static_png.bytes), 10_571_536_601_866_443_234);
    assert_eq!(fnv64(&player_png.bytes), 4_704_700_052_829_135_744);
    assert_ne!(static_png.bytes, player_png.bytes);
    let image = image::load_from_memory(&static_png.bytes)?.to_rgba8();
    let (_, _, width, height) =
        ink_bounds(&image, 200, 1_000, 1_175, 1_250).ok_or("music info credit was not rendered")?;
    assert!(width >= 630, "music info credit is too narrow: {width}");
    assert!(height >= 21, "music info credit is too short: {height}");
    Ok(())
}

#[test]
fn standard_note_row_leaves_touch_column_empty_and_keeps_break_column() -> Result<(), Box<dyn Error>>
{
    let cache = TempDir::new()?;
    let renderer = MusicInfoRenderer::new(static_root(), cache.path())?;
    let standard = renderer.render(&view(None, ChartGeneration::Standard)?)?;
    let deluxe = renderer.render(&view(None, ChartGeneration::Deluxe)?)?;
    let standard = image::load_from_memory(&standard.bytes)?.to_rgba8();
    let deluxe = image::load_from_memory(&deluxe.bytes)?.to_rgba8();
    assert_eq!(ink_pixels(&standard, 875, 950, 570, 630), 0);
    assert!(ink_pixels(&deluxe, 875, 950, 570, 630) > 0);
    assert!(ink_pixels(&standard, 990, 1_075, 570, 630) > 0);
    Ok(())
}

fn view(
    context: Option<PlayerSongScoreContext>,
    generation: ChartGeneration,
) -> Result<MusicInfoView, Box<dyn Error>> {
    let charts = vec![
        MusicInfoChart::new(
            Difficulty::Basic,
            "5",
            Some(ChartConstant::from_decimal_str("5.0")?),
            Some(ChartConstant::from_decimal_str("5.12")?),
            Some(NoteCounts {
                tap: 63,
                hold: 23,
                slide: 8,
                touch: 77,
                break_notes: 2,
            }),
            Some(173),
            "-",
        )?,
        MusicInfoChart::new(
            Difficulty::Master,
            "13+",
            Some(ChartConstant::from_decimal_str("13.7")?),
            Some(ChartConstant::from_decimal_str("13.42")?),
            Some(NoteCounts {
                tap: 600,
                hold: 80,
                slide: 120,
                touch: 30,
                break_notes: 20,
            }),
            Some(850),
            "譜面-100号",
        )?,
    ];
    Ok(MusicInfoView::new(
        Some(SourceSongId::numeric(SongIdNamespace::DivingFish, 835)),
        Some(835),
        "Believe the Rainbow",
        "Shoichiro Hirata feat.Sana",
        "maimai",
        "PRiSM",
        Some(128),
        Some(generation),
        true,
        None,
        charts,
    )?
    .with_score_context(context))
}

fn static_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../maimaidx_render_mcp/static")
}

fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn ink_pixels(image: &image::RgbaImage, x0: u32, x1: u32, y0: u32, y1: u32) -> usize {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| image.get_pixel(x, y)))
        .filter(|pixel| pixel.0 == [124, 130, 255, 255])
        .count()
}

fn ink_bounds(
    image: &image::RgbaImage,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
) -> Option<(u32, u32, u32, u32)> {
    let points = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|(x, y)| image.get_pixel(*x, *y).0 == [124, 130, 255, 255])
        .collect::<Vec<_>>();
    let min_x = points.iter().map(|(x, _)| *x).min()?;
    let max_x = points.iter().map(|(x, _)| *x).max()?;
    let min_y = points.iter().map(|(_, y)| *y).min()?;
    let max_y = points.iter().map(|(_, y)| *y).max()?;
    Some((min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
}
