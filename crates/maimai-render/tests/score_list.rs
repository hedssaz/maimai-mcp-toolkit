use std::path::PathBuf;

use maimai_core::{AchievementRate, ChartConstant, ChartGeneration, Difficulty, SongIdValue};
use maimai_render::{ScoreListItem, ScoreListRenderer, ScoreListView};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn complete_assets_render_legacy_dimensions_and_placeholder_pixel() -> TestResult {
    let temp = TempDir::new()?;
    let renderer = ScoreListRenderer::new(static_root(), temp.path())?;
    let rendered = renderer.render(&ScoreListView {
        target: "14.0".to_owned(),
        page: 1,
        pages: 1,
        total: 1,
        first: 1,
        items: vec![ScoreListItem {
            display_id: SongIdValue::Numeric(123_456_789),
            cover_id: SongIdValue::Numeric(123_456_789),
            image_name: None,
            title: "missing cover fixture".to_owned(),
            generation: ChartGeneration::Deluxe,
            difficulty: Difficulty::Master,
            constant: Some(ChartConstant::from_decimal_str("14.0")?),
            achievement: Some(AchievementRate::from_decimal_str("100.0000")?.into()),
            dx_score: Some(850),
            max_dx_score: Some(1_000),
            rating: Some(280),
            combo: None,
            sync: None,
        }],
    })?;
    assert_eq!((rendered.width, rendered.height), (1_400, 726));
    assert_eq!(rendered.placeholder_covers, 1);
    let image = image::load_from_memory(&rendered.bytes)?.to_rgba8();
    assert_eq!(image.get_pixel(65, 189).0, [224, 229, 245, 255]);
    assert_ne!(image.get_pixel(700, 642).0, [0, 0, 0, 0]);
    Ok(())
}

#[test]
fn empty_result_is_a_successful_legacy_sized_image() -> TestResult {
    let temp = TempDir::new()?;
    let rendered = ScoreListRenderer::new(static_root(), temp.path())?.render(&ScoreListView {
        target: "15".to_owned(),
        page: 1,
        pages: 1,
        total: 0,
        first: 0,
        items: Vec::new(),
    })?;
    assert_eq!((rendered.width, rendered.height), (1_400, 726));
    assert_eq!(rendered.placeholder_covers, 0);
    Ok(())
}

#[test]
fn incomplete_asset_root_returns_stable_assets_required_without_root_path() -> TestResult {
    let temp = TempDir::new()?;
    let root = temp.path().join("secret-static-root");
    std::fs::create_dir(&root)?;
    let error = ScoreListRenderer::new(&root, temp.path())
        .err()
        .ok_or("incomplete assets should fail")?;
    let text = error.to_string();
    assert!(error.assets_required());
    assert!(text.starts_with("missing score-list assets:"));
    assert!(!text.contains(root.to_string_lossy().as_ref()));
    Ok(())
}

fn static_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../maimaidx_render_mcp/static")
}
