use std::{error::Error, path::Path};

use image::GenericImageView;

use super::{
    RegionHeatmapAssets, RegionHeatmapRenderer, RegionHeatmapRow, RegionHeatmapView,
    color::heatmap_color,
    geometry::feature_names,
    layout::{LabelFont, LabelItem, label_bounds, place},
    projection::ranked_rows,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn native_asset_render_layout_and_callouts_match_frozen_behavior() -> TestResult {
    let assets = assets()?;
    let names = feature_names(&assets.features);
    assert_eq!(names.len(), 34);
    for name in ["台湾", "香港", "澳门"] {
        assert!(names.contains(&name));
    }
    let synthetic = vec![
        LabelItem {
            province: "山东".to_owned(),
            value: 99,
            center: (500.0, 300.0),
            offset: (0, 0),
            font: LabelFont::Normal,
            fill: image::Rgba([30, 41, 59, 255]),
            area: 10_000,
        },
        LabelItem {
            province: "江苏".to_owned(),
            value: 88,
            center: (500.0, 300.0),
            offset: (0, 0),
            font: LabelFont::Normal,
            fill: image::Rgba([30, 41, 59, 255]),
            area: 9_500,
        },
    ];
    let placement = place(synthetic, Vec::new(), &assets);
    assert_eq!(placement.labels.len(), 2);
    let bounds = placement
        .labels
        .iter()
        .map(|label| label.bounds)
        .collect::<Vec<_>>();
    assert!(bounds.iter().all(|bounds| bounds.inside(label_bounds())));
    assert!(!bounds[0].intersects(bounds[1], 2));

    let renderer = RegionHeatmapRenderer::new(assets);
    let empty = renderer.render(&RegionHeatmapView::new(Vec::new(), None)?)?;
    assert_png(&empty.bytes, 2_000, 1_440)?;
    assert!(empty.callout_regions.is_empty());

    let dense = renderer.render(&RegionHeatmapView::new(
        dense_rows()?,
        Some("缓存 2026-06-01 08:00:00".to_owned()),
    )?)?;
    assert_png(&dense.bytes, 2_000, 1_440)?;
    assert_eq!(dense.callout_regions, ["北京", "天津", "上海"]);

    let all = renderer.render(&RegionHeatmapView::new(all_official_rows()?, None)?)?;
    assert_png(&all.bytes, 2_000, 1_440)?;
    assert_eq!(
        all.callout_regions,
        ["北京", "天津", "上海", "海南", "宁夏"]
    );
    Ok(())
}

#[test]
fn colors_and_stable_tie_ranking_match_goldens() -> TestResult {
    assert_eq!(heatmap_color(0, 1_000, 800).0, [229, 236, 244, 255]);
    assert_eq!(heatmap_color(800, 1_000, 800).0, [219, 245, 255, 255]);
    assert_eq!(heatmap_color(900, 1_000, 800).0, [166, 206, 76, 255]);
    assert_eq!(heatmap_color(1_000, 1_000, 800).0, [190, 18, 60, 255]);

    let rows = vec![
        RegionHeatmapRow::new("山东", 10, None)?,
        RegionHeatmapRow::new("云南", 10, None)?,
        RegionHeatmapRow::new("北京", 20, None)?,
    ];
    assert_eq!(
        ranked_rows(&rows)
            .into_iter()
            .map(RegionHeatmapRow::province)
            .collect::<Vec<_>>(),
        ["北京", "山东", "云南"]
    );
    Ok(())
}

fn assets() -> TestResult<RegionHeatmapAssets> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Ok(RegionHeatmapAssets::load(
        root.join("assets/china_provinces.geojson"),
        root.join("maimaidx_render_mcp/static/ShangguMonoSC-Regular.otf"),
        root.join("maimaidx_render_mcp/static/ResourceHanRoundedCN-Bold.ttf"),
    )?)
}

fn dense_rows() -> Result<Vec<RegionHeatmapRow>, crate::RenderError> {
    [
        ("北京", 42),
        ("天津", 12),
        ("上海", 77),
        ("河北", 33),
        ("山东", 991),
        ("江苏", 66),
        ("浙江", 55),
        ("福建", 44),
    ]
    .into_iter()
    .map(|(province, count)| {
        RegionHeatmapRow::new(province, count, Some("2026-06-01 10:00:00".to_owned()))
    })
    .collect()
}

fn all_official_rows() -> Result<Vec<RegionHeatmapRow>, crate::RenderError> {
    let names = [
        "北京",
        "重庆",
        "上海",
        "天津",
        "安徽",
        "福建",
        "甘肃",
        "广东",
        "贵州",
        "海南",
        "河北",
        "黑龙江",
        "河南",
        "湖北",
        "湖南",
        "江苏",
        "江西",
        "吉林",
        "辽宁",
        "青海",
        "陕西",
        "山东",
        "山西",
        "四川",
        "西藏",
        "云南",
        "浙江",
        "广西",
        "内蒙古",
        "宁夏",
        "新疆",
    ];
    names
        .into_iter()
        .enumerate()
        .map(|(index, province)| {
            let count = 20 + (((index + 1) * 37) % 980) as u64;
            RegionHeatmapRow::new(province, count, Some("2026-06-01 12:00:00".to_owned()))
        })
        .collect()
}

fn assert_png(bytes: &[u8], width: u32, height: u32) -> TestResult {
    let decoded = image::load_from_memory(bytes)?;
    assert_eq!(decoded.dimensions(), (width, height));
    Ok(())
}
