use std::path::{Path, PathBuf};

use image::{ImageFormat, Rgba};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, Difficulty as CoreDifficulty, RatingBreakdown,
    SongIdNamespace, SourceSongId,
};
use maimai_render::{
    B50View, ChartType, Difficulty, LegacyAssets, LegacyRenderer, MissingCoverReason, PlayerHeader,
    RenderError, ScoreCard,
};

fn fixture_font() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/DejaVuSans-ASCII.ttf")
}

fn renderer() -> Result<LegacyRenderer, RenderError> {
    let font = fixture_font();
    LegacyRenderer::new(LegacyAssets::new(&font, &font))
}

fn score_card(
    id: u32,
    title: &str,
    chart_type: ChartType,
    level: &str,
    constant: &str,
    achievements: &str,
    rating: u32,
) -> Result<ScoreCard, Box<dyn std::error::Error>> {
    Ok(ScoreCard::new(
        Some(SourceSongId::numeric(SongIdNamespace::DivingFish, id)),
        title,
        chart_type,
        Difficulty::Master,
        level,
        Some(ChartConstant::from_decimal_str(constant)?),
        Some(AchievementRate::from_decimal_str(achievements)?),
        rating,
    )?)
}

fn two_card_view() -> Result<B50View, Box<dyn std::error::Error>> {
    let standard = score_card(
        1,
        "Static Song",
        ChartType::Standard,
        "13",
        "13.4",
        "100.1234",
        300,
    )?
    .with_markers(
        Some("sss".to_owned()),
        Some("fc".to_owned()),
        Some("fs".to_owned()),
    )?;
    let deluxe = score_card(
        10_002,
        "DX Song",
        ChartType::Deluxe,
        "13+",
        "13.8",
        "100.5678",
        320,
    )?
    .with_markers(
        Some("sssp".to_owned()),
        Some("fcp".to_owned()),
        Some("fsdp".to_owned()),
    )?;
    Ok(B50View::new(
        "maimai DX Best 50",
        PlayerHeader::new("Test Player", Some(620), Some("Test Plate".to_owned()))?,
        RatingBreakdown {
            b35: 300,
            b15: 320,
            total: 620,
        },
        vec![standard],
        vec![deluxe],
    )?)
}

#[test]
fn two_card_fixture_has_legacy_dimensions_png_and_stable_layout_pixels()
-> Result<(), Box<dyn std::error::Error>> {
    let rendered = renderer()?.render(&two_card_view()?)?;
    assert_eq!(&rendered.bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(rendered.metadata.width, 1_920);
    assert_eq!(rendered.metadata.height, 478);
    assert_eq!(rendered.metadata.card_count, 2);
    assert_eq!(rendered.metadata.missing_covers.len(), 2);
    assert!(
        rendered
            .metadata
            .missing_covers
            .iter()
            .all(|cover| cover.reason == MissingCoverReason::NotProvided)
    );

    let image = image::load_from_memory_with_format(&rendered.bytes, ImageFormat::Png)?.to_rgba8();
    assert_eq!(*image.get_pixel(0, 0), Rgba([34, 110, 180, 255]));
    assert_eq!(*image.get_pixel(0, 249), Rgba([70, 160, 225, 255]));
    assert_eq!(*image.get_pixel(0, 250), Rgba([246, 248, 252, 255]));
    assert_eq!(*image.get_pixel(106, 264), Rgba([48, 128, 208, 255]));
    assert_eq!(*image.get_pixel(480, 264), Rgba([226, 108, 70, 255]));
    assert_eq!(*image.get_pixel(122, 304), Rgba([226, 231, 239, 255]));
    Ok(())
}

#[test]
fn empty_b50_keeps_one_row_canvas() -> Result<(), Box<dyn std::error::Error>> {
    let view = B50View::new(
        "Empty B50",
        PlayerHeader::new("Nobody", None, None)?,
        RatingBreakdown {
            b35: 0,
            b15: 0,
            total: 0,
        },
        Vec::new(),
        Vec::new(),
    )?;
    let rendered = renderer()?.render(&view)?;
    assert_eq!(
        (rendered.metadata.width, rendered.metadata.height),
        (1_920, 478)
    );
    assert_eq!(rendered.metadata.card_count, 0);
    assert!(rendered.metadata.missing_covers.is_empty());
    Ok(())
}

#[test]
fn long_text_is_fitted_and_unreadable_cover_uses_placeholder()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let bad_cover = temporary.path().join("bad.png");
    std::fs::write(&bad_cover, b"not an image")?;
    let long_text = "A".repeat(2_000);
    let card = score_card(
        7,
        &long_text,
        ChartType::Standard,
        "14+",
        "14.6",
        "99.5000",
        250,
    )?
    .with_cover(bad_cover);
    let view = B50View::new(
        &long_text,
        PlayerHeader::new(&long_text, Some(15_000), Some(long_text.clone()))?,
        RatingBreakdown {
            b35: 250,
            b15: 0,
            total: 250,
        },
        vec![card],
        Vec::new(),
    )?;
    let rendered = renderer()?.render(&view)?;
    assert_eq!(rendered.metadata.missing_covers.len(), 1);
    assert_eq!(
        rendered.metadata.missing_covers[0].reason,
        MissingCoverReason::Unreadable
    );
    assert_eq!(
        (rendered.metadata.width, rendered.metadata.height),
        (1_920, 478)
    );
    Ok(())
}

#[test]
fn missing_or_invalid_font_is_structured() -> Result<(), Box<dyn std::error::Error>> {
    let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/missing.ttf");
    let error = LegacyRenderer::new(LegacyAssets::new(&missing, &missing))
        .err()
        .ok_or("expected missing font error")?;
    assert!(matches!(error, RenderError::AssetRead { .. }));

    let temporary = tempfile::tempdir()?;
    let invalid = temporary.path().join("invalid.ttf");
    std::fs::write(&invalid, b"not a font")?;
    let error = LegacyRenderer::new(LegacyAssets::new(&invalid, &invalid))
        .err()
        .ok_or("expected invalid font error")?;
    assert!(matches!(error, RenderError::InvalidAsset { .. }));
    Ok(())
}

#[test]
fn oversized_cover_and_background_are_rejected_before_decode_allocation()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let oversized_cover = temporary.path().join("oversized-cover.png");
    image::RgbaImage::new(2_049, 1).save_with_format(&oversized_cover, ImageFormat::Png)?;
    let card = score_card(
        9,
        "Oversized Cover",
        ChartType::Standard,
        "13",
        "13.0",
        "100.0000",
        200,
    )?
    .with_cover(oversized_cover);
    let view = B50View::new(
        "B50",
        PlayerHeader::new("Player", None, None)?,
        RatingBreakdown {
            b35: 200,
            b15: 0,
            total: 200,
        },
        vec![card],
        Vec::new(),
    )?;
    let error = renderer()?
        .render(&view)
        .err()
        .ok_or("expected oversized cover error")?;
    assert!(matches!(error, RenderError::InvalidAsset { .. }));

    let oversized_background = temporary.path().join("oversized-background.png");
    image::RgbaImage::new(4_097, 1).save_with_format(&oversized_background, ImageFormat::Png)?;
    let font = fixture_font();
    let error =
        LegacyRenderer::new(LegacyAssets::new(&font, &font).with_background(oversized_background))
            .err()
            .ok_or("expected oversized background error")?;
    assert!(matches!(error, RenderError::InvalidAsset { .. }));
    Ok(())
}

#[test]
fn invalid_model_and_utage_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    assert!(PlayerHeader::new("\n", None, None).is_err());
    assert!(PlayerHeader::new("Player", None, Some("bad\nplate".to_owned())).is_err());

    let marker_error = score_card(
        1,
        "Song",
        ChartType::Standard,
        "13",
        "13.0",
        "100.0000",
        200,
    )?
    .with_markers(Some("bad\ngrade".to_owned()), None, None)
    .err()
    .ok_or("expected invalid marker")?;
    assert!(matches!(marker_error, RenderError::InvalidModel { .. }));

    let card = score_card(
        1,
        "Song",
        ChartType::Standard,
        "13",
        "13.0",
        "100.0000",
        200,
    )?;
    let too_many = vec![card; 36];
    assert!(
        B50View::new(
            "B50",
            PlayerHeader::new("Player", None, None)?,
            RatingBreakdown {
                b35: 0,
                b15: 0,
                total: 0,
            },
            too_many,
            Vec::new(),
        )
        .is_err()
    );

    assert!(ChartType::try_from(ChartGeneration::UtageOnePlayer).is_err());
    assert!(Difficulty::try_from(CoreDifficulty::Utage).is_err());
    Ok(())
}
