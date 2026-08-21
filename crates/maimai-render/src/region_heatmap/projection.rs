use std::collections::HashMap;

use super::{
    drawing::RectBox,
    geometry::{GeoPoint, RegionFeature},
    model::RegionHeatmapRow,
};

const MAP: RectBox = RectBox {
    left: 48,
    top: 126,
    right: 740,
    bottom: 638,
};

pub(super) struct MapStats {
    pub maximum: u64,
    pub minimum_positive: u64,
}

pub(super) fn map_stats(rows: &[RegionHeatmapRow], features: &[RegionFeature]) -> MapStats {
    let counts = row_counts(rows);
    let values = features
        .iter()
        .map(|feature| counts.get(feature.name.as_str()).copied().unwrap_or(0))
        .collect::<Vec<_>>();
    MapStats {
        maximum: values.iter().copied().max().unwrap_or(0),
        minimum_positive: values
            .iter()
            .copied()
            .filter(|value| *value > 0)
            .min()
            .unwrap_or(0),
    }
}

pub(super) fn mapped_total(rows: &[RegionHeatmapRow], features: &[RegionFeature]) -> u64 {
    let counts = row_counts(rows);
    features
        .iter()
        .map(|feature| counts.get(feature.name.as_str()).copied().unwrap_or(0))
        .fold(0, u64::saturating_add)
}

pub(super) fn project(point: GeoPoint) -> (i32, i32) {
    let x = f64::from(MAP.left) + (point.longitude - 73.0) * f64::from(MAP.right - MAP.left) / 63.0;
    let y = f64::from(MAP.top) + (53.6 - point.latitude) * f64::from(MAP.bottom - MAP.top) / 36.0;
    (x as i32, y as i32)
}

pub(super) fn projected_center(feature: &RegionFeature, rings: &[Vec<(i32, i32)>]) -> (f64, f64) {
    if let Some(center) = feature.center {
        let center = project(center);
        return (f64::from(center.0), f64::from(center.1));
    }
    let largest = rings.iter().max_by_key(|ring| ring.len());
    let count = largest.map_or(0, |ring| ring.len());
    if count == 0 {
        return (0.0, 0.0);
    }
    let (x, y) = largest
        .into_iter()
        .flatten()
        .fold((0_i64, 0_i64), |sum, point| {
            (sum.0 + i64::from(point.0), sum.1 + i64::from(point.1))
        });
    (x as f64 / count as f64, y as f64 / count as f64)
}

pub(super) fn projected_area(rings: &[Vec<(i32, i32)>]) -> i64 {
    let points = rings.iter().flatten().collect::<Vec<_>>();
    let Some(first) = points.first() else {
        return 0;
    };
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.0, first.0, first.1, first.1);
    for point in points {
        min_x = min_x.min(point.0);
        max_x = max_x.max(point.0);
        min_y = min_y.min(point.1);
        max_y = max_y.max(point.1);
    }
    i64::from((max_x - min_x).max(0)) * i64::from((max_y - min_y).max(0))
}

pub(super) fn is_small_label(value: &str) -> bool {
    matches!(
        value,
        "北京" | "天津" | "上海" | "宁夏" | "海南" | "香港" | "澳门"
    )
}

pub(super) fn ranked_rows(rows: &[RegionHeatmapRow]) -> Vec<&RegionHeatmapRow> {
    let mut rows = rows.iter().collect::<Vec<_>>();
    rows.sort_by_key(|row| std::cmp::Reverse(row.play_count()));
    rows
}

fn row_counts(rows: &[RegionHeatmapRow]) -> HashMap<&str, u64> {
    rows.iter()
        .map(|row| (row.province(), row.play_count()))
        .collect()
}
