use std::cmp::Ordering;

use ab_glyph::FontArc;
use image::Rgba;

use super::{
    assets::RegionHeatmapAssets,
    drawing::{RectBox, text_dimensions},
};

const LABEL_BOUNDS: RectBox = RectBox {
    left: 54,
    top: 130,
    right: 734,
    bottom: 634,
};
const LABEL_OFFSETS: [(i32, i32); 11] = [
    (0, 0),
    (0, -18),
    (0, 18),
    (20, 0),
    (-20, 0),
    (22, -16),
    (-22, -16),
    (22, 16),
    (-22, 16),
    (0, -32),
    (0, 32),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LabelFont {
    Normal,
    Small,
}

#[derive(Clone, Debug)]
pub(super) struct LabelItem {
    pub province: String,
    pub value: u64,
    pub center: (f64, f64),
    pub offset: (i32, i32),
    pub font: LabelFont,
    pub fill: Rgba<u8>,
    pub area: i64,
}

#[derive(Clone, Debug)]
pub(super) struct CalloutItem {
    pub province: String,
    pub value: u64,
    pub anchor: (f64, f64),
    pub fill: Rgba<u8>,
}

#[derive(Clone, Debug)]
pub(super) struct PlacedLabel {
    pub item: LabelItem,
    pub center: (f64, f64),
    pub show_count: bool,
    #[cfg(test)]
    pub bounds: RectBox,
}

#[derive(Clone, Debug)]
pub(super) struct PlacedCallout {
    pub item: CalloutItem,
    pub position: (i32, i32),
    pub bounds: RectBox,
}

#[derive(Clone, Debug)]
pub(super) struct Placement {
    pub labels: Vec<PlacedLabel>,
    pub callouts: Vec<PlacedCallout>,
}

pub(super) fn place(
    mut labels: Vec<LabelItem>,
    callouts: Vec<CalloutItem>,
    assets: &RegionHeatmapAssets,
) -> Placement {
    labels.sort_by(|left, right| compare_labels(right, left));
    let mut placed_boxes = Vec::new();
    let mut placed_labels = Vec::new();
    for item in labels {
        if let Some((center, bounds, show_count)) = place_label(&item, &placed_boxes, assets) {
            placed_boxes.push(bounds);
            placed_labels.push(PlacedLabel {
                item,
                center,
                show_count,
                #[cfg(test)]
                bounds,
            });
        }
    }
    let mut placed_callouts = Vec::new();
    for (province, initial_offset) in callout_offsets() {
        let Some(item) = callouts.iter().find(|item| item.province == province) else {
            continue;
        };
        if let Some((position, bounds)) = place_callout(
            item,
            initial_offset,
            &placed_boxes,
            &assets.bold,
            &assets.regular,
        ) {
            placed_boxes.push(bounds);
            placed_callouts.push(PlacedCallout {
                item: item.clone(),
                position,
                bounds,
            });
        }
    }
    Placement {
        labels: placed_labels,
        callouts: placed_callouts,
    }
}

fn compare_labels(left: &LabelItem, right: &LabelItem) -> Ordering {
    layout_priority(&left.province)
        .cmp(&layout_priority(&right.province))
        .then_with(|| left.area.cmp(&right.area))
        .then_with(|| (left.value > 0).cmp(&(right.value > 0)))
}

fn place_label(
    item: &LabelItem,
    placed: &[RectBox],
    assets: &RegionHeatmapAssets,
) -> Option<((f64, f64), RectBox, bool)> {
    let (font, size) = label_font(item.font, assets);
    let count_modes: &[bool] = if item.value > 0 {
        &[true, false]
    } else {
        &[false]
    };
    for show_count in count_modes {
        for offset in LABEL_OFFSETS {
            let center = candidate_center(item, offset);
            let bounds = label_box(
                center,
                &item.province,
                item.value,
                font,
                size,
                &assets.regular,
                *show_count,
            );
            if bounds.inside(LABEL_BOUNDS)
                && !placed.iter().any(|other| bounds.intersects(*other, 2))
            {
                return Some((center, bounds, *show_count));
            }
        }
    }

    let mut best: Option<(i32, (f64, f64), RectBox)> = None;
    for offset in LABEL_OFFSETS {
        let center = candidate_center(item, offset);
        let bounds = label_box(
            center,
            &item.province,
            item.value,
            font,
            size,
            &assets.regular,
            false,
        );
        if !bounds.inside(LABEL_BOUNDS) {
            continue;
        }
        let overlap = placed
            .iter()
            .map(|other| bounds.overlap_area(*other))
            .fold(0_i32, i32::saturating_add);
        if best.as_ref().is_none_or(|current| overlap < current.0) {
            best = Some((overlap, center, bounds));
        }
    }
    best.filter(|(overlap, _, _)| *overlap == 0 || item.area >= 9_000)
        .map(|(_, center, bounds)| (center, bounds, false))
}

fn place_callout(
    item: &CalloutItem,
    initial: (i32, i32),
    placed: &[RectBox],
    label_font: &FontArc,
    count_font: &FontArc,
) -> Option<((i32, i32), RectBox)> {
    let offsets = [
        initial,
        (initial.0, initial.1 + 36),
        (initial.0 + 14, initial.1 - 36),
        (34, -42),
        (34, 4),
        (34, 42),
        (-92, -42),
        (-92, 4),
        (-92, 42),
        (8, -58),
    ];
    for offset in offsets {
        let position = (
            item.anchor.0 as i32 + offset.0,
            item.anchor.1 as i32 + offset.1,
        );
        let bounds = callout_box(position, &item.province, item.value, label_font, count_font);
        if bounds.inside(LABEL_BOUNDS) && !placed.iter().any(|other| bounds.intersects(*other, 2)) {
            return Some((position, bounds));
        }
    }
    None
}

pub(super) fn label_box(
    center: (f64, f64),
    province: &str,
    value: u64,
    font: &FontArc,
    font_size: f32,
    count_font: &FontArc,
    show_count: bool,
) -> RectBox {
    let (label_width, label_height) = text_dimensions(province, font_size, font);
    let x = center.0 as i32;
    let y = center.1 as i32;
    let mut bounds = RectBox {
        left: x - label_width / 2 - 4,
        top: y - label_height / 2 - 3,
        right: x + label_width / 2 + 4,
        bottom: y + label_height / 2 + 3,
    };
    if show_count {
        let (count_width, count_height) = text_dimensions(&value.to_string(), 12.0, count_font);
        bounds.left = bounds.left.min(x - count_width / 2 - 4);
        bounds.right = bounds.right.max(x + count_width / 2 + 4);
        bounds.top = bounds.top.min(y + 13 - count_height / 2 - 3);
        bounds.bottom = bounds.bottom.max(y + 13 + count_height / 2 + 3);
    }
    bounds
}

pub(super) fn callout_box(
    position: (i32, i32),
    province: &str,
    value: u64,
    label_font: &FontArc,
    count_font: &FontArc,
) -> RectBox {
    let (label_width, label_height) = text_dimensions(province, 13.0, label_font);
    let (count_width, count_height) = text_dimensions(&format!("{value}次"), 11.0, count_font);
    RectBox {
        left: position.0,
        top: position.1,
        right: position.0 + label_width.max(count_width) + 34,
        bottom: position.1 + 34.max(label_height + count_height + 12),
    }
}

pub(super) fn label_font(role: LabelFont, assets: &RegionHeatmapAssets) -> (&FontArc, f32) {
    match role {
        LabelFont::Normal => (&assets.bold, 15.0),
        LabelFont::Small => (&assets.bold, 12.0),
    }
}

fn candidate_center(item: &LabelItem, extra: (i32, i32)) -> (f64, f64) {
    (
        item.center.0 + f64::from(item.offset.0 + extra.0),
        item.center.1 + f64::from(item.offset.1 + extra.1),
    )
}

pub(super) fn label_offset(province: &str) -> (i32, i32) {
    match province {
        "北京" => (-6, -8),
        "天津" => (8, 8),
        "上海" => (18, 0),
        "江苏" => (16, -16),
        "浙江" => (12, 8),
        "重庆" => (0, 8),
        "宁夏" => (0, -6),
        "海南" => (0, 16),
        "台湾" => (18, 0),
        "香港" => (20, 14),
        "澳门" => (-16, 20),
        _ => (0, 0),
    }
}

pub(super) fn uses_callout(province: &str) -> bool {
    callout_offsets()
        .iter()
        .any(|(candidate, _)| *candidate == province)
}

fn callout_offsets() -> [(&'static str, (i32, i32)); 5] {
    [
        ("北京", (18, -126)),
        ("天津", (92, -14)),
        ("上海", (56, -18)),
        ("海南", (-92, -34)),
        ("宁夏", (-94, -46)),
    ]
}

fn layout_priority(province: &str) -> u8 {
    u8::from(matches!(
        province,
        "山西" | "陕西" | "山东" | "江苏" | "安徽"
    ))
}

#[cfg(test)]
pub(super) const fn label_bounds() -> RectBox {
    LABEL_BOUNDS
}
