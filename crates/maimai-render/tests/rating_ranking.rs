use std::{error::Error, fs, path::PathBuf};

use image::ImageReader;
use maimai_render::{RatingRankingDocument, RatingRankingRenderer};
use tempfile::TempDir;

#[test]
fn fixed_document_has_stable_dimensions_and_visible_pixels() -> Result<(), Box<dyn Error>> {
    let temp = assets()?;
    let rendered =
        RatingRankingRenderer::new(temp.path())?.render(&RatingRankingDocument::new(vec![
            "Rating Ranking".to_owned(),
            "No.01 16000 Alice".to_owned(),
        ])?)?;
    let image = ImageReader::new(std::io::Cursor::new(&rendered.bytes))
        .with_guessed_format()?
        .decode()?
        .to_rgba8();
    assert_eq!(
        (image.width(), image.height()),
        (rendered.width, rendered.height)
    );
    assert_eq!((rendered.width, rendered.height), (295, 82));
    assert!(image.pixels().any(|pixel| pixel.0 != [255, 255, 255, 255]));
    Ok(())
}

#[test]
fn long_username_is_ellipsized_within_canvas_limit() -> Result<(), Box<dyn Error>> {
    let temp = assets()?;
    let line = format!("No.01 16000 {}", "x".repeat(128));
    let rendered = RatingRankingRenderer::new(temp.path())?
        .render(&RatingRankingDocument::new(vec![line])?)?;
    assert!(rendered.width <= 1_200);
    assert!(rendered.width > 160);
    Ok(())
}

#[test]
fn invalid_document_and_missing_font_fail_closed() -> Result<(), Box<dyn Error>> {
    assert!(RatingRankingDocument::new(vec!["bad\nline".to_owned()]).is_err());
    assert!(RatingRankingDocument::new(vec!["x".repeat(257)]).is_err());
    let temp = TempDir::new()?;
    assert!(RatingRankingRenderer::new(temp.path()).is_err());
    Ok(())
}

fn assets() -> Result<TempDir, Box<dyn Error>> {
    let temp = TempDir::new()?;
    fs::copy(
        workspace_root().join("crates/maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf"),
        temp.path().join("ShangguMonoSC-Regular.otf"),
    )?;
    Ok(temp)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
