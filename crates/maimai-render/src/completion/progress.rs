use ab_glyph::FontArc;
use image::{
    Rgba, RgbaImage,
    imageops::{FilterType, overlay, resize},
};
use imageproc::drawing::text_size;
use maimai_core::{FullComboStatus as CoreCombo, FullSyncStatus as CoreSync};

use super::{
    CompletionRenderer,
    cells::draw_cover_id_cell,
    encode,
    model::{
        CompletionRenderedPng, FullComboStatus, FullSyncStatus, LevelProgressView, ProgressPage,
        ScoreCardCell,
    },
};
use crate::{
    RenderError,
    score_list::{ScoreListItem, card::ScoreCardDrawer, layout::gradient},
    text::{draw_text, fit_text, middle_origin_y},
};

const WIDTH: u32 = 1_400;
const TEXT_COLOR: Rgba<u8> = Rgba([124, 129, 255, 255]);

impl CompletionRenderer {
    pub fn render_level_progress(
        &self,
        view: &LevelProgressView,
    ) -> Result<CompletionRenderedPng, RenderError> {
        view.validate()?;
        let height = canvas_height(view);
        let mut image = gradient(height);
        self.draw_background(&mut image);
        let mut placeholders = 0;
        match view.page {
            ProgressPage::Overview => {
                let mut y = 30;
                y = self.draw_score_section(
                    &mut image,
                    y,
                    "已完成谱面",
                    &view.completed,
                    &mut placeholders,
                )?;
                y = self.draw_score_section(
                    &mut image,
                    y,
                    "未完成谱面",
                    &view.unfinished,
                    &mut placeholders,
                )?;
                self.draw_cover_section(
                    &mut image,
                    y,
                    "未游玩谱面",
                    &view.not_started,
                    &mut placeholders,
                )?;
            }
            ProgressPage::Completed { .. } => {
                self.draw_page(&mut image, "已完成谱面", &view.completed, &mut placeholders)?
            }
            ProgressPage::Unfinished { .. } => self.draw_page(
                &mut image,
                "未完成谱面",
                &view.unfinished,
                &mut placeholders,
            )?,
            ProgressPage::NotStarted => {
                self.draw_not_started_page(&mut image, &view.not_started, &mut placeholders)?
            }
        }
        self.draw_footer(&mut image, view);
        let (bytes, width, height) = encode(image)?;
        Ok(CompletionRenderedPng {
            bytes,
            width,
            height,
            placeholder_covers: placeholders,
        })
    }

    pub fn render_text_panel(&self, text: &str) -> Result<CompletionRenderedPng, RenderError> {
        if text.trim().is_empty()
            || text
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        {
            return Err(RenderError::invalid(
                "completion.text",
                "must be non-empty and contain no unsupported control characters",
            ));
        }
        let lines = text.trim().lines().collect::<Vec<_>>();
        let font_size = 35.0;
        let line_height = 36_u32;
        let padding = 10_u32;
        let width = lines
            .iter()
            .map(|line| text_size(font_size, &self.assets.mono, line).0)
            .max()
            .unwrap_or_default()
            .saturating_add(padding * 2);
        let height = u32::try_from(lines.len())
            .unwrap_or(u32::MAX)
            .saturating_mul(line_height)
            .saturating_add(padding * 2)
            .saturating_sub(4);
        let mut image = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
        for (index, line) in lines.into_iter().enumerate() {
            draw_text(
                &mut image,
                (
                    i32::try_from(padding).unwrap_or(10),
                    i32::try_from(padding).unwrap_or(10) + index as i32 * line_height as i32,
                ),
                line,
                font_size,
                Rgba([0, 0, 0, 255]),
                &self.assets.mono,
            );
        }
        let (bytes, width, height) = encode(image)?;
        Ok(CompletionRenderedPng {
            bytes,
            width,
            height,
            placeholder_covers: 0,
        })
    }

    fn draw_background(&self, image: &mut RgbaImage) {
        overlay(
            image,
            &resize(&self.score_assets.aurora, WIDTH, 220, FilterType::Lanczos3),
            0,
            0,
        );
        overlay(image, &self.score_assets.shines, 34, 0);
        overlay(
            image,
            &self.score_assets.rainbow,
            319,
            i64::from(image.height().saturating_sub(643)),
        );
        overlay(
            image,
            &resize(
                &self.score_assets.rainbow_bottom,
                1_200,
                200,
                FilterType::Lanczos3,
            ),
            100,
            i64::from(image.height().saturating_sub(343)),
        );
        let mut y = 0_u32;
        while y < image.height() {
            overlay(image, &self.score_assets.pattern, 0, i64::from(y));
            y = y.saturating_add(365);
        }
    }

    fn draw_score_section(
        &self,
        image: &mut RgbaImage,
        y: i32,
        title: &str,
        cells: &[ScoreCardCell],
        placeholders: &mut usize,
    ) -> Result<i32, RenderError> {
        self.draw_section_title(image, y, title, cells.len());
        let drawer = ScoreCardDrawer::new(&self.score_assets, &self.covers);
        for (index, cell) in cells.iter().enumerate() {
            let x = 16 + (index % 5) as i32 * 276;
            let card_y = y + 110 + (index / 5) as i32 * 114;
            *placeholders += usize::from(
                drawer
                    .draw(image, (x, card_y), &score_item(cell))
                    .map_err(score_render_error)?,
            );
        }
        Ok(y + 140 + cells.len().div_ceil(5) as i32 * 109)
    }

    fn draw_cover_section(
        &self,
        image: &mut RgbaImage,
        y: i32,
        title: &str,
        cells: &[ScoreCardCell],
        placeholders: &mut usize,
    ) -> Result<(), RenderError> {
        self.draw_section_title(image, y, title, cells.len());
        for (index, cell) in cells.iter().enumerate() {
            let x = 55 + (index % 20) as i32 * 65;
            let cell_y = y + 110 + (index / 20) as i32 * 65;
            *placeholders += usize::from(draw_cover_id_cell(
                image,
                (x, cell_y),
                cell,
                &self.assets,
                &self.covers,
            )?);
        }
        Ok(())
    }

    fn draw_page(
        &self,
        image: &mut RgbaImage,
        title: &str,
        cells: &[ScoreCardCell],
        placeholders: &mut usize,
    ) -> Result<(), RenderError> {
        overlay(image, &self.score_assets.title, 475, 30);
        centered_text(
            image,
            (700, 77),
            title,
            38.0,
            TEXT_COLOR,
            &self.score_assets.bold,
        );
        let drawer = ScoreCardDrawer::new(&self.score_assets, &self.covers);
        for (index, cell) in cells.iter().enumerate() {
            let x = 16 + (index % 5) as i32 * 276;
            let y = 140 + (index / 5) as i32 * 114;
            *placeholders += usize::from(
                drawer
                    .draw(image, (x, y), &score_item(cell))
                    .map_err(score_render_error)?,
            );
        }
        Ok(())
    }

    fn draw_not_started_page(
        &self,
        image: &mut RgbaImage,
        cells: &[ScoreCardCell],
        placeholders: &mut usize,
    ) -> Result<(), RenderError> {
        overlay(image, &self.score_assets.title, 475, 30);
        centered_text(
            image,
            (700, 77),
            "未游玩谱面",
            38.0,
            TEXT_COLOR,
            &self.score_assets.bold,
        );
        for (index, cell) in cells.iter().enumerate() {
            let x = 55 + (index % 20) as i32 * 65;
            let y = 200 + (index / 20) as i32 * 65;
            *placeholders += usize::from(draw_cover_id_cell(
                image,
                (x, y),
                cell,
                &self.assets,
                &self.covers,
            )?);
        }
        Ok(())
    }

    fn draw_section_title(&self, image: &mut RgbaImage, y: i32, title: &str, count: usize) {
        overlay(image, &self.score_assets.title, 475, i64::from(y));
        centered_text(
            image,
            (700, y + 47),
            &format!("{title}「{count}」个"),
            32.0,
            TEXT_COLOR,
            &self.score_assets.bold,
        );
    }

    fn draw_footer(&self, image: &mut RgbaImage, view: &LevelProgressView) {
        let height = image.height();
        overlay(
            image,
            &self.score_assets.design,
            200,
            i64::from(height.saturating_sub(113)),
        );
        let footer = match view.page {
            ProgressPage::Overview => format!(
                "共计「{}」个谱面，剩余「{}」个谱面未完成「{}」",
                view.total, view.remaining, view.target
            ),
            ProgressPage::Completed { page, pages } => {
                format!("已完成谱面，当前第「{page} / {pages}」页")
            }
            ProgressPage::Unfinished { page, pages } => {
                format!("未完成谱面，当前第「{page} / {pages}」页")
            }
            ProgressPage::NotStarted => {
                format!("未游玩谱面共计「{}」个", view.not_started.len())
            }
        };
        centered_text(
            image,
            (
                700,
                i32::try_from(height.saturating_sub(70)).unwrap_or(i32::MAX),
            ),
            &fit_text(&footer, 930, 36.0, &self.score_assets.bold),
            36.0,
            TEXT_COLOR,
            &self.score_assets.bold,
        );
    }
}

fn score_item(cell: &ScoreCardCell) -> ScoreListItem {
    ScoreListItem {
        display_id: cell.cover_id.clone(),
        cover_id: cell.cover_id.clone(),
        image_name: cell.image_name.clone(),
        title: cell.title.clone(),
        generation: cell.generation,
        difficulty: cell.difficulty,
        constant: cell.constant,
        achievement: cell.achievement.map(Into::into),
        dx_score: cell.dx_score,
        max_dx_score: cell.max_dx_score,
        rating: cell.rating,
        combo: cell.combo.map(core_combo),
        sync: cell.sync.map(core_sync),
    }
}

const fn core_combo(value: FullComboStatus) -> CoreCombo {
    match value {
        FullComboStatus::FullCombo => CoreCombo::FullCombo,
        FullComboStatus::FullComboPlus => CoreCombo::FullComboPlus,
        FullComboStatus::AllPerfect => CoreCombo::AllPerfect,
        FullComboStatus::AllPerfectPlus => CoreCombo::AllPerfectPlus,
    }
}

const fn core_sync(value: FullSyncStatus) -> CoreSync {
    match value {
        FullSyncStatus::FullSync => CoreSync::FullSync,
        FullSyncStatus::FullSyncPlus => CoreSync::FullSyncPlus,
        FullSyncStatus::FullSyncDeluxe => CoreSync::FullSyncDeluxe,
        FullSyncStatus::FullSyncDeluxePlus => CoreSync::FullSyncDeluxePlus,
    }
}

fn score_render_error(error: crate::score_list::ScoreListRenderError) -> RenderError {
    match error {
        crate::score_list::ScoreListRenderError::Render(error) => error,
        crate::score_list::ScoreListRenderError::AssetsRequired { missing } => {
            RenderError::AssetsRequired {
                style: "completion",
                missing,
            }
        }
        crate::score_list::ScoreListRenderError::InvalidAsset { name } => {
            RenderError::AssetsRequired {
                style: "completion",
                missing: vec![name],
            }
        }
    }
}

fn centered_text(
    image: &mut RgbaImage,
    center: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &FontArc,
) {
    let (width, _) = text_size(size, font, text);
    crate::text::draw_text(
        image,
        (
            center.0 - i32::try_from(width).unwrap_or(i32::MAX) / 2,
            middle_origin_y(center.1, text, size, font),
        ),
        text,
        size,
        color,
        font,
    );
}

fn canvas_height(view: &LevelProgressView) -> u32 {
    match view.page {
        ProgressPage::Overview => {
            570 + view.completed.len().div_ceil(5) as u32 * 109
                + view.unfinished.len().div_ceil(5) as u32 * 109
                + view.not_started.len().div_ceil(20) as u32 * 65
        }
        ProgressPage::Completed { .. } => 360 + view.completed.len().div_ceil(5) as u32 * 109,
        ProgressPage::Unfinished { .. } => 360 + view.unfinished.len().div_ceil(5) as u32 * 109,
        ProgressPage::NotStarted => 360 + view.not_started.len().div_ceil(20) as u32 * 65,
    }
}
