use std::{
    error::Error,
    fs::{self, File},
    path::{Path, PathBuf},
};

use image::Rgba;
use maimai_core::Difficulty;
use maimai_render::{MusicGlobalStatsRenderer, MusicGlobalStatsView};
use tempfile::TempDir;

#[test]
fn fixed_canvas_and_donut_pixel_anchors_are_stable() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let renderer = renderer(&temp)?;
    let mut achievements = [0_u64; 14];
    achievements[1] = 100;
    let view = MusicGlobalStatsView::new(
        Some(641),
        "Calamity Fortune",
        Difficulty::Master,
        achievements,
        [100, 0, 0, 0, 0],
    )?;

    let rendered = renderer.render(&view)?;
    assert_eq!((rendered.width, rendered.height), (1_000, 800));
    assert_eq!(rendered.bytes.get(..8), Some(&b"\x89PNG\r\n\x1a\n"[..]));
    let image = image::load_from_memory(&rendered.bytes)?.to_rgba8();
    assert_eq!(*image.get_pixel(280, 200), Rgba([226, 232, 240, 255]));
    assert_eq!(*image.get_pixel(720, 200), Rgba([94, 234, 212, 255]));
    assert_eq!(*image.get_pixel(280, 365), Rgba([255, 255, 255, 255]));
    assert_eq!(*image.get_pixel(720, 365), Rgba([255, 255, 255, 255]));
    Ok(())
}

#[test]
fn empty_distributions_long_titles_and_invalid_models_are_bounded() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let renderer = renderer(&temp)?;
    let view = MusicGlobalStatsView::new(
        None,
        "a title that is intentionally much longer than the available title area ".repeat(20),
        Difficulty::ReMaster,
        [0; 14],
        [0; 5],
    )?;
    let rendered = renderer.render(&view)?;
    assert_eq!((rendered.width, rendered.height), (1_000, 800));

    assert!(
        MusicGlobalStatsView::new(None, "bad\ntitle", Difficulty::Master, [0; 14], [0; 5]).is_err()
    );
    assert!(MusicGlobalStatsView::new(None, "utage", Difficulty::Utage, [0; 14], [0; 5]).is_err());
    Ok(())
}

#[test]
fn missing_font_is_a_structured_asset_error() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let error = MusicGlobalStatsRenderer::new(temp.path())
        .err()
        .ok_or("missing fonts should fail")?;
    assert!(
        error
            .to_string()
            .contains("music global stats numeric font")
    );
    Ok(())
}

#[test]
fn oversized_font_is_rejected_before_reading_it() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let root = temp.path().join("static");
    fs::create_dir(&root)?;
    File::create(root.join("Torus SemiBold.otf"))?.set_len(32 * 1024 * 1024 + 1)?;
    fs::copy(fixture_font(), root.join("ResourceHanRoundedCN-Bold.ttf"))?;
    let error = MusicGlobalStatsRenderer::new(root)
        .err()
        .ok_or("oversized font should fail")?;
    assert!(
        error
            .to_string()
            .contains("invalid music global stats numeric font")
    );
    Ok(())
}

fn renderer(temp: &TempDir) -> Result<MusicGlobalStatsRenderer, Box<dyn Error>> {
    let root = temp.path().join("static");
    fs::create_dir(&root)?;
    let fixture = fixture_font();
    fs::copy(&fixture, root.join("Torus SemiBold.otf"))?;
    fs::copy(&fixture, root.join("ResourceHanRoundedCN-Bold.ttf"))?;
    Ok(MusicGlobalStatsRenderer::new(root)?)
}

fn fixture_font() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/DejaVuSans-ASCII.ttf")
}
