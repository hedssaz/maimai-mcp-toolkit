use std::{error::Error, path::PathBuf};

use maimai_app::{
    music_global_stats::{
        MusicGlobalStatsBatchResult, MusicGlobalStatsImage, MusicGlobalStatsItemError,
    },
    music_info::MusicInfoChartType,
};
use serde_json::{Value, json};

use super::{TOOL_NAME, convert, dto::MusicGlobalStatsArgs, format};

#[test]
fn frozen_main_and_public_schemas_are_identical() -> Result<(), Box<dyn Error>> {
    let main = crate::contract::SurfaceContract::parse(crate::render_tools::MAIN_CONTRACT_JSON)?
        .retain_tools(&[TOOL_NAME]);
    let public =
        crate::contract::SurfaceContract::parse(crate::render_tools::PUBLIC_CONTRACT_JSON)?
            .retain_tools(&[TOOL_NAME]);
    assert_eq!(main.tools().len(), 1);
    assert_eq!(public.tools().len(), 1);
    assert_eq!(main.tools()[0].name(), TOOL_NAME);
    assert_eq!(
        main.tools()[0].input_schema(),
        public.tools()[0].input_schema()
    );
    Ok(())
}

#[test]
fn dto_preserves_difficulty_and_index_aliases() -> Result<(), Box<dyn Error>> {
    for (value, expected) in [
        (json!({"query":"song","difficulty":"紫"}), 3),
        (json!({"query":"song","diff":"re:master"}), 4),
        (json!({"query":"song","levelIndex":"2"}), 2),
        (json!({"query":"song","difficulty_index":1}), 1),
    ] {
        let args: MusicGlobalStatsArgs = serde_json::from_value(value)?;
        let request = convert::request(args)?;
        assert_eq!(request.difficulty.index(), expected);
    }
    let invalid: MusicGlobalStatsArgs =
        serde_json::from_value(json!({"query":"song","level_index":5}))?;
    assert_eq!(
        convert::request(invalid)
            .err()
            .map(|error| error.to_string())
            .as_deref(),
        Some("level_index/difficulty_index 必须是 0-4")
    );
    Ok(())
}

#[test]
fn single_and_partial_batch_formats_keep_legacy_shapes() -> Result<(), Box<dyn Error>> {
    let image = MusicGlobalStatsImage {
        index: 1,
        query: "Calamity Fortune".to_owned(),
        music_id: "641".to_owned(),
        title: "Calamity Fortune".to_owned(),
        chart_type: MusicInfoChartType::Standard,
        level_index: 3,
        image_path: PathBuf::from("/tmp/stats.png"),
        width: 1_000,
        height: 800,
    };
    let single = MusicGlobalStatsBatchResult {
        images: vec![image.clone()],
        errors: Vec::new(),
    };
    assert_eq!(
        format::single(&single)?,
        r#"{"imagePath":"/tmp/stats.png","mimeType":"image/png","width":1000,"height":800}"#
    );

    let partial = MusicGlobalStatsBatchResult {
        images: vec![image],
        errors: vec![MusicGlobalStatsItemError {
            index: 2,
            query: "Calamity Fortune".to_owned(),
            chart_type: Some(MusicInfoChartType::Deluxe),
            kind: maimai_app::music_global_stats::MusicGlobalStatsFailureKind::Input,
            message: "全服统计 dist 必须恰好包含 14 项".to_owned(),
        }],
    };
    let value: Value = serde_json::from_str(&format::single(&partial)?)?;
    assert_eq!(value["results"], value["images"]);
    assert_eq!(value["images"][0]["chartType"], "ST");
    assert_eq!(value["images"][0]["levelIndex"], 3);
    assert_eq!(value["errors"][0]["index"], "2");

    let input_error = partial.errors[0].clone();
    assert_eq!(
        format::single_error(&MusicGlobalStatsBatchResult {
            images: Vec::new(),
            errors: vec![input_error],
        })
        .as_deref(),
        Some("全服统计 dist 必须恰好包含 14 项")
    );
    assert_eq!(
        format::single_error(&MusicGlobalStatsBatchResult {
            images: Vec::new(),
            errors: vec![MusicGlobalStatsItemError {
                index: 1,
                query: "Calamity Fortune".to_owned(),
                chart_type: Some(MusicInfoChartType::Standard),
                kind: maimai_app::music_global_stats::MusicGlobalStatsFailureKind::Render,
                message: "font missing".to_owned(),
            }],
        })
        .as_deref(),
        Some("渲染全服统计图失败: font missing")
    );
    Ok(())
}
