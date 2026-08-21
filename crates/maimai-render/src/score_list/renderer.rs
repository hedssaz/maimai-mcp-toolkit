use std::{io::Cursor, path::Path};

use image::{
    ColorType, ImageEncoder, Rgba, RgbaImage,
    codecs::png::PngEncoder,
    imageops::{FilterType, overlay, resize},
};

use super::{
    ScoreListRenderError, ScoreListRenderedPng, ScoreListView,
    assets::ScoreListAssets,
    card::ScoreCardDrawer,
    layout::{GROUP_SIZE, WIDTH, canvas_height, card_origin, draw_centered, gradient, group_y},
};
use crate::CoverResolver;

const TEXT_COLOR: Rgba<u8> = Rgba([124, 129, 255, 255]);

pub struct ScoreListRenderer {
    assets: ScoreListAssets,
    covers: CoverResolver,
}

impl ScoreListRenderer {
    pub fn new(
        static_root: impl AsRef<Path>,
        cover_cache_root: impl AsRef<Path>,
    ) -> Result<Self, ScoreListRenderError> {
        Ok(Self {
            assets: ScoreListAssets::load(static_root.as_ref())?,
            covers: CoverResolver::new(static_root.as_ref(), cover_cache_root.as_ref()),
        })
    }

    pub fn render(
        &self,
        view: &ScoreListView,
    ) -> Result<ScoreListRenderedPng, ScoreListRenderError> {
        view.validate()?;
        let height = canvas_height(view.items.len());
        let mut image = gradient(height);
        self.draw_background(&mut image);
        let cards = ScoreCardDrawer::new(&self.assets, &self.covers);
        let mut placeholder_covers = 0;
        for (group, items) in view.items.chunks(GROUP_SIZE).enumerate() {
            let y = group_y(group);
            overlay(&mut image, &self.assets.title, 475, i64::from(30 + y));
            let start = view.first + group * GROUP_SIZE;
            let end = start + items.len().saturating_sub(1);
            draw_centered(
                &mut image,
                700,
                60 + y,
                &format!("No.{start}- No.{end}"),
                41.0,
                TEXT_COLOR,
                &self.assets.bold,
            );
            for (index, item) in items.iter().enumerate() {
                placeholder_covers +=
                    usize::from(cards.draw(&mut image, card_origin(y, index), item)?);
            }
        }
        self.draw_footer(&mut image, view);
        encode(image, placeholder_covers)
    }

    fn draw_background(&self, image: &mut RgbaImage) {
        overlay(
            image,
            &resize(&self.assets.aurora, WIDTH, 220, FilterType::Lanczos3),
            0,
            0,
        );
        overlay(image, &self.assets.shines, 34, 0);
        overlay(
            image,
            &self.assets.rainbow,
            319,
            i64::from(image.height().saturating_sub(643)),
        );
        overlay(
            image,
            &resize(
                &self.assets.rainbow_bottom,
                1_200,
                200,
                FilterType::Lanczos3,
            ),
            100,
            i64::from(image.height().saturating_sub(343)),
        );
        let mut y = 0_u32;
        while y < image.height() {
            overlay(image, &self.assets.pattern, 0, i64::from(y));
            y = y.saturating_add(365);
        }
    }

    fn draw_footer(&self, image: &mut RgbaImage, view: &ScoreListView) {
        let height = image.height();
        overlay(
            image,
            &self.assets.design,
            200,
            i64::from(height.saturating_sub(113)),
        );
        let (first, last) = if view.items.is_empty() {
            (0, 0)
        } else {
            (view.first, view.first + view.items.len() - 1)
        };
        let footer = format!(
            "「{}」共计「{}」个成绩，展示第「{}-{}」个，当前第「{} / {}」页",
            view.target, view.total, first, last, view.page, view.pages
        );
        draw_centered(
            image,
            700,
            i32::try_from(height.saturating_sub(84)).unwrap_or(0),
            &footer,
            36.0,
            TEXT_COLOR,
            &self.assets.bold,
        );
    }
}

fn encode(
    image: RgbaImage,
    placeholder_covers: usize,
) -> Result<ScoreListRenderedPng, ScoreListRenderError> {
    let width = image.width();
    let height = image.height();
    let mut bytes = Vec::new();
    PngEncoder::new(Cursor::new(&mut bytes))
        .write_image(image.as_raw(), width, height, ColorType::Rgba8.into())
        .map_err(crate::RenderError::PngEncode)?;
    Ok(ScoreListRenderedPng {
        bytes,
        width,
        height,
        placeholder_covers,
    })
}
