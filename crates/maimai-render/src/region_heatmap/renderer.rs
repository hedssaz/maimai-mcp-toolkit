use std::{collections::HashMap, io::Cursor};

use image::{
    ColorType, ImageEncoder, Rgba, RgbaImage,
    codecs::png::PngEncoder,
    imageops::{FilterType, resize},
};
use imageproc::drawing::draw_filled_circle_mut;

use super::{
    RegionHeatmapAssets,
    color::heatmap_color,
    drawing::{
        RectBox, centered_text, draw_text, filled_rounded_rect, line, outlined_rounded_rect,
        polygon, text_dimensions,
    },
    layout::{
        CalloutItem, LabelFont, LabelItem, PlacedCallout, PlacedLabel, label_font, label_offset,
        place, uses_callout,
    },
    model::{RegionHeatmapRenderedPng, RegionHeatmapRow, RegionHeatmapView},
    projection::{
        MapStats, is_small_label, map_stats, mapped_total, project, projected_area,
        projected_center, ranked_rows,
    },
};
use crate::RenderError;

const WIDTH: u32 = 1_000;
const HEIGHT: u32 = 720;
const OUTPUT_WIDTH: u32 = 2_000;
const OUTPUT_HEIGHT: u32 = 1_440;
const INK: Rgba<u8> = Rgba([15, 23, 42, 255]);
const TEXT: Rgba<u8> = Rgba([30, 41, 59, 255]);
const MUTED: Rgba<u8> = Rgba([100, 116, 139, 255]);
const BORDER: Rgba<u8> = Rgba([226, 232, 240, 255]);
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

pub struct RegionHeatmapRenderer {
    assets: RegionHeatmapAssets,
}

impl RegionHeatmapRenderer {
    pub fn new(assets: RegionHeatmapAssets) -> Self {
        Self { assets }
    }

    pub fn render(
        &self,
        view: &RegionHeatmapView,
    ) -> Result<RegionHeatmapRenderedPng, RenderError> {
        let mut image = RgbaImage::from_pixel(WIDTH, HEIGHT, Rgba([248, 250, 252, 255]));
        outlined_rounded_rect(
            &mut image,
            RectBox {
                left: 28,
                top: 26,
                right: 972,
                bottom: 694,
            },
            26,
            WHITE,
            BORDER,
            2,
        );
        self.draw_header(&mut image, view);
        let stats = map_stats(view.rows(), &self.assets.features);
        let (labels, callouts) = self.draw_map(&mut image, view.rows(), &stats);
        let placement = place(labels, callouts, &self.assets);
        for label in &placement.labels {
            self.draw_label(&mut image, label);
        }
        for callout in &placement.callouts {
            self.draw_callout(&mut image, callout);
        }
        self.draw_legend(&mut image, &stats);
        self.draw_ranking(&mut image, view.rows(), &stats);
        let callout_regions = placement
            .callouts
            .iter()
            .map(|callout| callout.item.province.clone())
            .collect();
        encode(image, callout_regions)
    }

    fn draw_header(&self, image: &mut RgbaImage, view: &RegionHeatmapView) {
        draw_text(
            image,
            (56, 44),
            "地区游玩热力图",
            34.0,
            INK,
            &self.assets.bold,
        );
        let mut parts = vec![
            format!("{} 个省份", view.rows().len()),
            format!(
                "合计 {} 次",
                mapped_total(view.rows(), &self.assets.features)
            ),
        ];
        if let Some(status) = view.status_label() {
            parts.push(status.to_owned());
        }
        draw_text(
            image,
            (58, 88),
            &parts.join(" / "),
            16.0,
            MUTED,
            &self.assets.regular,
        );
    }

    fn draw_map(
        &self,
        image: &mut RgbaImage,
        rows: &[RegionHeatmapRow],
        stats: &MapStats,
    ) -> (Vec<LabelItem>, Vec<CalloutItem>) {
        let counts = rows
            .iter()
            .map(|row| (row.province(), row.play_count()))
            .collect::<HashMap<_, _>>();
        let mut labels = Vec::new();
        let mut callouts = Vec::new();
        for feature in &self.assets.features {
            let value = counts.get(feature.name.as_str()).copied().unwrap_or(0);
            let fill = heatmap_color(value, stats.maximum, stats.minimum_positive);
            let projected = feature
                .rings
                .iter()
                .map(|ring| ring.iter().copied().map(project).collect::<Vec<_>>())
                .filter(|ring| ring.len() >= 3)
                .collect::<Vec<_>>();
            for ring in &projected {
                polygon(image, ring, fill);
                for pair in ring.windows(2) {
                    line(
                        image,
                        (pair[0].0 as f32, pair[0].1 as f32),
                        (pair[1].0 as f32, pair[1].1 as f32),
                        WHITE,
                        2,
                    );
                }
                if let (Some(first), Some(last)) = (ring.first(), ring.last()) {
                    line(
                        image,
                        (last.0 as f32, last.1 as f32),
                        (first.0 as f32, first.1 as f32),
                        WHITE,
                        2,
                    );
                }
            }
            let center = projected_center(feature, &projected);
            if value > 0 && uses_callout(&feature.name) {
                callouts.push(CalloutItem {
                    province: feature.name.clone(),
                    value,
                    anchor: center,
                    fill,
                });
            } else {
                labels.push(LabelItem {
                    province: feature.name.clone(),
                    value,
                    center,
                    offset: label_offset(&feature.name),
                    font: if is_small_label(&feature.name) {
                        LabelFont::Small
                    } else {
                        LabelFont::Normal
                    },
                    fill: if value > 0 { TEXT } else { MUTED },
                    area: projected_area(&projected),
                });
            }
        }
        (labels, callouts)
    }

    fn draw_label(&self, image: &mut RgbaImage, placed: &PlacedLabel) {
        let (font, size) = label_font(placed.item.font, &self.assets);
        centered_text(
            image,
            (
                placed.center.0,
                placed.center.1 - if placed.show_count { 6.0 } else { 0.0 },
            ),
            &placed.item.province,
            size,
            placed.item.fill,
            font,
            Some((Rgba([255, 255, 255, 220]), 2)),
        );
        if placed.show_count {
            centered_text(
                image,
                (placed.center.0, placed.center.1 + 13.0),
                &placed.item.value.to_string(),
                12.0,
                INK,
                &self.assets.regular,
                Some((Rgba([255, 255, 255, 220]), 2)),
            );
        }
    }

    fn draw_callout(&self, image: &mut RgbaImage, placed: &PlacedCallout) {
        let box_ = placed.bounds;
        let anchor = (placed.item.anchor.0 as i32, placed.item.anchor.1 as i32);
        let connect_x = if anchor.0 < box_.left {
            box_.left
        } else {
            box_.right
        };
        let connect_y = anchor.1.clamp(box_.top + 8, box_.bottom - 8);
        line(
            image,
            (anchor.0 as f32, anchor.1 as f32),
            (connect_x as f32, connect_y as f32),
            Rgba([255, 255, 255, 230]),
            3,
        );
        line(
            image,
            (anchor.0 as f32, anchor.1 as f32),
            (connect_x as f32, connect_y as f32),
            Rgba([71, 85, 105, 170]),
            1,
        );
        draw_filled_circle_mut(image, anchor, 3, Rgba([71, 85, 105, 170]));
        outlined_rounded_rect(
            image,
            box_,
            8,
            Rgba([255, 255, 255, 245]),
            Rgba([148, 163, 184, 255]),
            1,
        );
        filled_rounded_rect(
            image,
            RectBox {
                left: placed.position.0 + 7,
                top: placed.position.1 + 8,
                right: placed.position.0 + 20,
                bottom: placed.position.1 + 21,
            },
            3,
            placed.item.fill,
        );
        draw_text(
            image,
            (placed.position.0 + 25, placed.position.1 + 5),
            &placed.item.province,
            13.0,
            TEXT,
            &self.assets.bold,
        );
        draw_text(
            image,
            (placed.position.0 + 25, placed.position.1 + 20),
            &format!("{}次", placed.item.value),
            11.0,
            MUTED,
            &self.assets.regular,
        );
    }

    fn draw_legend(&self, image: &mut RgbaImage, stats: &MapStats) {
        draw_text(image, (92, 656), "少", 13.0, MUTED, &self.assets.regular);
        for index in 0..120_i32 {
            let value = if stats.maximum == 0 {
                0
            } else {
                stats
                    .minimum_positive
                    .saturating_add((stats.maximum - stats.minimum_positive) * index as u64 / 119)
            };
            line(
                image,
                ((126 + index) as f32, 666.0),
                ((126 + index) as f32, 682.0),
                heatmap_color(value, stats.maximum.max(1), stats.minimum_positive),
                1,
            );
        }
        draw_text(image, (254, 656), "多", 13.0, MUTED, &self.assets.regular);
        draw_text(
            image,
            (306, 656),
            &format!("最高 {} 次", stats.maximum),
            13.0,
            MUTED,
            &self.assets.regular,
        );
    }

    fn draw_ranking(&self, image: &mut RgbaImage, rows: &[RegionHeatmapRow], stats: &MapStats) {
        outlined_rounded_rect(
            image,
            RectBox {
                left: 760,
                top: 126,
                right: 948,
                bottom: 662,
            },
            18,
            Rgba([248, 250, 252, 255]),
            BORDER,
            2,
        );
        draw_text(image, (784, 152), "省份排行", 21.0, INK, &self.assets.bold);
        line(image, (784.0, 184.0), (924.0, 184.0), BORDER, 2);
        let ranked = ranked_rows(rows);
        if ranked.is_empty() {
            draw_text(
                image,
                (784, 210),
                "暂无地区记录",
                15.0,
                MUTED,
                &self.assets.regular,
            );
        }
        for (offset, row) in ranked.into_iter().take(13).enumerate() {
            let index = offset + 1;
            let y = 208 + offset as i32 * 34;
            filled_rounded_rect(
                image,
                RectBox {
                    left: 784,
                    top: y - 4,
                    right: 806,
                    bottom: y + 18,
                },
                5,
                heatmap_color(row.play_count(), stats.maximum, stats.minimum_positive),
            );
            let label = format!("{index}. {} {}次", row.province(), row.play_count());
            let size = if text_dimensions(&label, 15.0, &self.assets.regular).0 <= 110 {
                15.0
            } else {
                13.0
            };
            draw_text(
                image,
                (814, y - 8),
                &label,
                size,
                TEXT,
                &self.assets.regular,
            );
            if let Some(first) = row.first_play_time().filter(|value| *value != "未知") {
                let date = first.chars().take(10).collect::<String>();
                draw_text(
                    image,
                    (814, y + 12),
                    &date,
                    13.0,
                    MUTED,
                    &self.assets.regular,
                );
            }
        }
    }
}

fn encode(
    image: RgbaImage,
    callout_regions: Vec<String>,
) -> Result<RegionHeatmapRenderedPng, RenderError> {
    let resized = resize(&image, OUTPUT_WIDTH, OUTPUT_HEIGHT, FilterType::Lanczos3);
    let rgb = image::DynamicImage::ImageRgba8(resized).to_rgb8();
    let mut bytes = Vec::new();
    PngEncoder::new(&mut Cursor::new(&mut bytes))
        .write_image(
            rgb.as_raw(),
            OUTPUT_WIDTH,
            OUTPUT_HEIGHT,
            ColorType::Rgb8.into(),
        )
        .map_err(RenderError::PngEncode)?;
    Ok(RegionHeatmapRenderedPng {
        bytes,
        width: OUTPUT_WIDTH,
        height: OUTPUT_HEIGHT,
        callout_regions,
    })
}
