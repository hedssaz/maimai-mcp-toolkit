use std::path::PathBuf;

use image::{GenericImageView, ImageReader};
use maimai_core::{AchievementRate, ChartConstant, ChartGeneration, Difficulty, SongIdValue};
use tempfile::TempDir;

use super::{
    CompletionRenderer, CompletionState, FullComboStatus, FullSyncStatus, LevelProgressView,
    PlateChartState, PlateMemberView, PlateTableKind, PlateTableView, ProgressPage, ScoreCardCell,
};

fn renderer(cache: &TempDir) -> Result<CompletionRenderer, crate::RenderError> {
    CompletionRenderer::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../maimaidx_render_mcp/static"),
        cache.path(),
    )
}

fn member() -> PlateMemberView {
    PlateMemberView {
        cover_id: SongIdValue::Numeric(8),
        image_name: None,
        title: "True Love Song".to_owned(),
        generation: ChartGeneration::Standard,
        master_level: "12".to_owned(),
        charts: vec![PlateChartState {
            difficulty: Difficulty::Master,
            state: CompletionState {
                completed: true,
                achievement: None,
                combo: Some(FullComboStatus::AllPerfectPlus),
                sync: None,
            },
        }],
    }
}

#[test]
fn real_cn_template_and_generated_custom_fallback_have_stable_png_anchors()
-> Result<(), Box<dyn std::error::Error>> {
    let cache = TempDir::new()?;
    let renderer = renderer(&cache)?;
    let cn = renderer.render_plate(&PlateTableView {
        kind: PlateTableKind::China,
        version: "真".to_owned(),
        target: "极".to_owned(),
        declared_song_count: 1,
        members: vec![member()],
    })?;
    assert_eq!((cn.width, cn.height), (1_400, 2_110));
    assert_eq!(
        ImageReader::new(std::io::Cursor::new(&cn.bytes))
            .with_guessed_format()?
            .decode()?
            .dimensions(),
        (1_400, 2_110)
    );

    let custom = renderer.render_plate(&PlateTableView {
        kind: PlateTableKind::Custom,
        version: "不存在的fixture牌".to_owned(),
        target: "将".to_owned(),
        declared_song_count: 1,
        members: vec![member()],
    })?;
    assert_eq!(custom.width, 1_400);
    assert_eq!(custom.height, 625);
    assert_eq!(custom.placeholder_covers, 0);
    Ok(())
}

#[test]
fn long_plate_progress_text_renders_as_a_real_png() -> Result<(), Box<dyn std::error::Error>> {
    let cache = TempDir::new()?;
    let rendered = renderer(&cache)?.render_text_panel("进度如下：\nNo.01 曲目\nNo.02 曲目")?;
    assert_eq!((rendered.width, rendered.height), (163, 124));
    assert!(
        rendered
            .bytes
            .starts_with(&[137, 80, 78, 71, 13, 10, 26, 10])
    );
    Ok(())
}

#[test]
fn overview_progress_keeps_the_original_section_offsets() -> Result<(), Box<dyn std::error::Error>>
{
    let cache = TempDir::new()?;
    let cell = ScoreCardCell {
        cover_id: SongIdValue::Numeric(8),
        image_name: None,
        title: "相信彩虹 Visual Review".to_owned(),
        generation: ChartGeneration::Standard,
        difficulty: Difficulty::Master,
        level: "13+".to_owned(),
        constant: Some(ChartConstant::from_decimal_str("13.7")?),
        achievement: Some(AchievementRate::from_decimal_str("100.1234")?),
        dx_score: Some(1_002),
        max_dx_score: Some(1_050),
        rating: Some(278),
        grade: Some("SSS+".to_owned()),
        combo: Some(FullComboStatus::AllPerfectPlus),
        sync: Some(FullSyncStatus::FullSyncDeluxePlus),
    };
    let rendered = renderer(&cache)?.render_level_progress(&LevelProgressView {
        level: "13+".to_owned(),
        target: "SSS+".to_owned(),
        page: ProgressPage::Overview,
        total: 18,
        remaining: 13,
        completed: vec![cell.clone(); 5],
        unfinished: vec![cell.clone(); 5],
        not_started: vec![cell; 8],
    })?;
    assert_eq!((rendered.width, rendered.height), (1_400, 853));
    Ok(())
}
