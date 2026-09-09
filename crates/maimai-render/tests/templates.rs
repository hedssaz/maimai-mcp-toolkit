use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use image::{ImageFormat, Rgba, RgbaImage};
use maimai_core::{AchievementRate, ChartConstant, RatingBreakdown, SongIdNamespace, SourceSongId};
use maimai_render::{
    B50View, ChartType, CoverResolver, Difficulty, LegacyAssets, LegacyRenderer, MaibotRenderer,
    PlayerHeader, RenderError, ScoreCard, YuzuRenderer,
};

fn fixture_font() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/DejaVuSans-ASCII.ttf")
}

fn legacy() -> Result<LegacyRenderer, RenderError> {
    let font = fixture_font();
    LegacyRenderer::new(LegacyAssets::new(&font, &font))
}

fn view(title: &str) -> Result<B50View, Box<dyn Error>> {
    let card = |id, title, chart_type, constant, achievement, rating| {
        Ok::<_, Box<dyn Error>>(
            ScoreCard::new(
                Some(SourceSongId::numeric(SongIdNamespace::DivingFish, id)),
                title,
                chart_type,
                Difficulty::Master,
                "13+",
                Some(ChartConstant::from_decimal_str(constant)?),
                Some(AchievementRate::from_decimal_str(achievement)?),
                rating,
            )?
            .with_markers(
                Some("sssp".to_owned()),
                Some("fcp".to_owned()),
                Some("fsdp".to_owned()),
            )?,
        )
    };
    Ok(B50View::new(
        title,
        PlayerHeader::new("Test Player", Some(620), Some("Test Plate".to_owned()))?,
        RatingBreakdown {
            b35: 300,
            b15: 320,
            total: 620,
        },
        vec![card(
            1,
            "Static Song",
            ChartType::Standard,
            "13.4",
            "100.1234",
            300,
        )?],
        vec![card(
            10_002,
            "DX Song",
            ChartType::Deluxe,
            "13.8",
            "100.5678",
            320,
        )?],
    )?)
}

#[test]
fn yuzu_and_maibot_keep_dimensions_layout_and_local_cover_priority() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let static_root = create_assets(temporary.path())?;
    let cache = temporary.path().join("cache");
    fs::create_dir(&cache)?;
    image(&cache.join("00002.png"), 64, 64, [20, 40, 220, 255])?;
    let covers = CoverResolver::new(&static_root, &cache);
    let view = view("Template Fixture")?;

    let yuzu_renderer = YuzuRenderer::new(&static_root)?;
    let yuzu = yuzu_renderer.render(&view, &covers)?;
    assert_eq!((yuzu.metadata.width, yuzu.metadata.height), (1_400, 1_650));
    assert!(yuzu.metadata.missing_covers.is_empty());
    let yuzu_image = image::load_from_memory_with_format(&yuzu.bytes, ImageFormat::Png)?.to_rgba8();
    assert_eq!(*yuzu_image.get_pixel(40, 260), Rgba([10, 200, 20, 255]));
    assert_eq!(*yuzu_image.get_pixel(40, 1_110), Rgba([220, 30, 20, 255]));
    assert_eq!(
        *yuzu_image.get_pixel(1_399, 1_649),
        Rgba([230, 235, 240, 255])
    );
    fs::remove_file(static_root.join("mai/pic/b50_bg.png"))?;
    assert_eq!(yuzu_renderer.render(&view, &covers)?.metadata.width, 1_400);

    let legacy = legacy()?;
    let legacy_output = legacy.render_with_covers(&view, &covers)?;
    assert!(legacy_output.metadata.missing_covers.is_empty());
    let legacy_image =
        image::load_from_memory_with_format(&legacy_output.bytes, ImageFormat::Png)?.to_rgba8();
    assert_eq!(*legacy_image.get_pixel(80, 300), Rgba([10, 200, 20, 255]));
    let maibot_renderer = MaibotRenderer::new(&static_root, &legacy)?;
    let maibot = maibot_renderer.render(&view, &covers)?;
    assert_eq!(
        (maibot.metadata.width, maibot.metadata.height),
        (1_400, 780)
    );
    assert!(maibot.metadata.missing_covers.is_empty());
    let maibot_image =
        image::load_from_memory_with_format(&maibot.bytes, ImageFormat::Png)?.to_rgba8();
    assert_eq!(*maibot_image.get_pixel(120, 200), Rgba([7, 144, 14, 255]));
    assert_eq!(
        *maibot_image.get_pixel(1_100, 200),
        Rgba([158, 21, 14, 255])
    );
    fs::remove_file(static_root.join("mai/pic/UI_TTR_BG_Base_Plus.png"))?;
    assert_eq!(
        maibot_renderer.render(&view, &covers)?.metadata.width,
        1_400
    );
    Ok(())
}

#[test]
fn missing_template_assets_are_style_specific() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let yuzu = YuzuRenderer::new(temporary.path())
        .err()
        .ok_or("expected yuzu assets error")?;
    assert!(matches!(
        yuzu,
        RenderError::AssetsRequired { style: "yuzu", .. }
    ));
    let maibot = MaibotRenderer::new(temporary.path(), &legacy()?)
        .err()
        .ok_or("expected maibot assets error")?;
    assert!(matches!(
        maibot,
        RenderError::AssetsRequired {
            style: "maibot",
            ..
        }
    ));
    Ok(())
}

#[test]
fn cover_placeholder_corruption_limits_and_long_text_are_safe() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let static_root = create_assets(temporary.path())?;
    fs::remove_file(static_root.join("mai/cover/00001.png"))?;
    fs::remove_file(static_root.join("mai/cover/10002.png"))?;
    fs::write(static_root.join("mai/cover/1.png"), b"not an image")?;
    let cache = temporary.path().join("empty-cache");
    fs::create_dir(&cache)?;
    let covers = CoverResolver::new(&static_root, &cache);
    let long = "A".repeat(2_000);
    let rendered = YuzuRenderer::new(&static_root)?.render(&view(&long)?, &covers)?;
    assert_eq!(rendered.metadata.missing_covers.len(), 2);

    image(
        &static_root.join("mai/cover/00001.png"),
        2_049,
        1,
        [0, 0, 0, 255],
    )?;
    let error = YuzuRenderer::new(&static_root)?
        .render(&view("Oversized")?, &covers)
        .err()
        .ok_or("expected oversized cover error")?;
    assert!(matches!(error, RenderError::InvalidAsset { .. }));

    image(
        &static_root.join("mai/pic/b50_bg.png"),
        4_097,
        1,
        [0, 0, 0, 255],
    )?;
    let error = YuzuRenderer::new(&static_root)
        .err()
        .ok_or("expected oversized background error")?;
    assert!(matches!(error, RenderError::InvalidAsset { .. }));
    Ok(())
}

#[test]
fn all_b50_styles_render_real_whitespace_title_and_unformatted_display_text()
-> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let static_root = create_assets(temporary.path())?;
    let covers = CoverResolver::new(&static_root, temporary.path().join("cache"));
    let card = ScoreCard::new(
        Some(SourceSongId::numeric(SongIdNamespace::DivingFish, 11_422)),
        "　",
        ChartType::Deluxe,
        Difficulty::Expert,
        "",
        None,
        Some(AchievementRate::from_decimal_str("98.3999")?),
        200,
    )?
    .with_markers(
        Some(" unknown\ngrade ".to_owned()),
        None,
        Some("sync".to_owned()),
    )?;
    assert_eq!(card.title(), "　");
    assert_eq!(card.level(), "");
    let header = PlayerHeader::new("　", Some(200), Some("\t".to_owned()))?;
    assert_eq!(header.nickname(), "　");
    assert_eq!(header.plate(), Some("\t"));
    let view = B50View::new(
        "Display text regression",
        header,
        RatingBreakdown {
            b35: 200,
            b15: 0,
            total: 200,
        },
        vec![card],
        vec![],
    )?;
    let legacy = legacy()?;
    for rendered in [
        legacy.render_with_covers(&view, &covers)?,
        YuzuRenderer::new(&static_root)?.render(&view, &covers)?,
        MaibotRenderer::new(&static_root, &legacy)?.render(&view, &covers)?,
    ] {
        assert_eq!(rendered.metadata.card_count, 1);
        assert_eq!(rendered.metadata.missing_covers[0].title, "　");
        let png = image::load_from_memory_with_format(&rendered.bytes, ImageFormat::Png)?;
        assert_eq!(png.width(), rendered.metadata.width);
        assert_eq!(png.height(), rendered.metadata.height);
    }
    Ok(())
}

#[test]
fn real_maibot_assets_render_the_original_canvas() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let static_root = root.join("maimaidx_render_mcp/static");
    let legacy_font = static_root.join("adobe_simhei.otf");
    let legacy = LegacyRenderer::new(LegacyAssets::new(&legacy_font, &legacy_font))?;
    let card = |chart_type| -> Result<ScoreCard, Box<dyn Error>> {
        Ok(ScoreCard::new(
            Some(SourceSongId::numeric(SongIdNamespace::DivingFish, 8)),
            "相信彩虹 Visual Review",
            chart_type,
            Difficulty::Master,
            "13+",
            Some(ChartConstant::from_decimal_str("13.7")?),
            Some(AchievementRate::from_decimal_str("100.1234")?),
            278,
        )?
        .with_markers(
            Some("sssp".to_owned()),
            Some("app".to_owned()),
            Some("fsdp".to_owned()),
        )?)
    };
    let standard = card(ChartType::Standard)?;
    let deluxe = card(ChartType::Deluxe)?;
    let view = B50View::new(
        "B50 Visual Review",
        PlayerHeader::new("Test Player", Some(16_000), Some("舞神".to_owned()))?,
        RatingBreakdown {
            b35: 10_000,
            b15: 6_000,
            total: 16_000,
        },
        vec![standard; 35],
        vec![deluxe; 15],
    )?;
    let cache = tempfile::tempdir()?;
    let rendered = MaibotRenderer::new(&static_root, &legacy)?
        .render(&view, &CoverResolver::new(&static_root, cache.path()))?;
    assert_eq!(
        (rendered.metadata.width, rendered.metadata.height),
        (1_406, 598)
    );
    Ok(())
}

fn create_assets(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let static_root = root.join("static");
    let pic = static_root.join("mai/pic");
    let cover = static_root.join("mai/cover");
    fs::create_dir_all(&pic)?;
    fs::create_dir_all(&cover)?;
    fs::copy(
        fixture_font(),
        static_root.join("ResourceHanRoundedCN-Bold.ttf"),
    )?;
    fs::copy(fixture_font(), static_root.join("Torus SemiBold.otf"))?;
    for (name, width, height, color) in [
        ("b50_bg.png", 1_400, 1_650, [230, 235, 240, 255]),
        ("b50_score_basic.png", 265, 105, [70, 180, 80, 255]),
        ("b50_score_advanced.png", 265, 105, [220, 180, 70, 255]),
        ("b50_score_expert.png", 265, 105, [220, 90, 100, 255]),
        ("b50_score_master.png", 265, 105, [130, 70, 190, 255]),
        ("b50_score_remaster.png", 265, 105, [210, 180, 230, 255]),
        ("logo.png", 249, 120, [50, 100, 180, 255]),
        ("Name.png", 170, 38, [255, 255, 255, 255]),
        ("UI_CMN_DXRating_10.png", 220, 50, [40, 120, 200, 255]),
        ("UI_CMN_DXRating_11.png", 220, 50, [40, 120, 200, 255]),
        ("UI_CMN_Shougou_Rainbow.png", 270, 27, [220, 240, 255, 255]),
        ("UI_DNM_DaniPlate_00.png", 120, 48, [200, 220, 255, 255]),
        ("UI_FBR_Class_00.png", 110, 70, [180, 200, 230, 255]),
        ("UI_Icon_309503.png", 120, 120, [180, 200, 230, 255]),
        ("UI_Plate_300501.png", 800, 130, [255, 255, 255, 255]),
        ("UI_TTR_Rank_SSS.png", 180, 80, [255, 210, 80, 255]),
        ("UI_TTR_Rank_SSSp.png", 180, 80, [255, 230, 100, 255]),
        ("SD.png", 70, 30, [130, 130, 170, 255]),
        ("DX.png", 70, 30, [230, 130, 90, 255]),
        ("UI_TTR_BG_Base_Plus.png", 1_400, 780, [245, 248, 252, 255]),
        (
            "UI_CMN_TabTitle_MaimaiTitle_Ver214.png",
            380,
            180,
            [120, 170, 230, 255],
        ),
        ("UI_CMN_DXRating_S_10.png", 220, 55, [40, 120, 200, 255]),
        ("UI_CMN_Name_DX.png", 55, 25, [40, 120, 200, 255]),
        ("UI_TST_PlateMask.png", 300, 48, [255, 255, 255, 255]),
        ("UI_RSL_MBase_Parts_01.png", 120, 60, [90, 170, 230, 255]),
        ("UI_RSL_MBase_Parts_02.png", 120, 60, [230, 160, 90, 255]),
        ("UI_GAM_Rank_SSSp.png", 180, 80, [255, 230, 100, 255]),
    ] {
        image(&pic.join(name), width, height, color)?;
    }
    image(&cover.join("00001.png"), 64, 64, [10, 200, 20, 255])?;
    image(&cover.join("10002.png"), 64, 64, [220, 30, 20, 255])?;
    Ok(static_root)
}

fn image(path: &Path, width: u32, height: u32, color: [u8; 4]) -> Result<(), image::ImageError> {
    RgbaImage::from_pixel(width, height, Rgba(color)).save_with_format(path, ImageFormat::Png)
}
