use image::{
    Rgba, RgbaImage,
    imageops::{FilterType, overlay, resize},
};
use maimai_core::{AchievementRank, ChartGeneration, Difficulty, SongIdValue, achievement_rank};

use super::{assets::RiseScoreAssets, layout::draw_centered, model::RiseScoreCandidate};
use crate::{
    CoverResolver,
    cover::ResolvedCover,
    text::{draw_text, fit_text, format_achievement, format_constant, middle_origin_y},
};

pub(super) struct RiseScoreCardDrawer<'a> {
    assets: &'a RiseScoreAssets,
    covers: &'a CoverResolver,
}

impl<'a> RiseScoreCardDrawer<'a> {
    pub(super) const fn new(assets: &'a RiseScoreAssets, covers: &'a CoverResolver) -> Self {
        Self { assets, covers }
    }

    pub(super) fn draw(
        &self,
        image: &mut RgbaImage,
        (x, y): (i32, i32),
        item: &RiseScoreCandidate,
    ) -> Result<bool, crate::RenderError> {
        let difficulty = difficulty_index(item.difficulty());
        overlay(
            image,
            &self.assets.cards[difficulty],
            i64::from(x + 30),
            i64::from(y),
        );
        let (cover, placeholder) = match self
            .covers
            .resolve_music(Some(&item.cover_id), item.image_name.as_deref())?
        {
            ResolvedCover::Image(image) => (image, false),
            ResolvedCover::Missing(_) => (
                RgbaImage::from_pixel(80, 80, Rgba([224, 229, 245, 255])),
                true,
            ),
        };
        overlay(
            image,
            &resize(&cover, 80, 80, FilterType::Lanczos3),
            i64::from(x + 55),
            i64::from(y + 40),
        );
        self.draw_icons(image, (x, y), item);
        self.draw_labels(image, (x, y), item, difficulty);
        Ok(placeholder)
    }

    fn draw_icons(&self, image: &mut RgbaImage, (x, y): (i32, i32), item: &RiseScoreCandidate) {
        let generation = if item.generation() == ChartGeneration::Standard {
            &self.assets.standard
        } else {
            &self.assets.deluxe
        };
        overlay(
            image,
            &resize(generation, 60, 22, FilterType::Lanczos3),
            i64::from(x + 240),
            i64::from(y + 114),
        );
        if let Some(old) = item.old_achievement {
            draw_rank(
                image,
                &self.assets.ranks[rank_index(achievement_rank(old))],
                (x + 145, y + 82),
            );
        }
        draw_rank(
            image,
            &self.assets.ranks[rank_index(achievement_rank(item.target_achievement))],
            (x + 305, y + 82),
        );
    }

    fn draw_labels(
        &self,
        image: &mut RgbaImage,
        (x, y): (i32, i32),
        item: &RiseScoreCandidate,
        difficulty: usize,
    ) {
        let colors = [
            Rgba([129, 217, 85, 255]),
            Rgba([245, 189, 21, 255]),
            Rgba([255, 129, 141, 255]),
            Rgba([159, 81, 220, 255]),
            Rgba([138, 0, 226, 255]),
        ];
        let text_colors = [
            Rgba([255, 255, 255, 255]),
            Rgba([255, 255, 255, 255]),
            Rgba([255, 255, 255, 255]),
            Rgba([255, 255, 255, 255]),
            Rgba([138, 0, 226, 255]),
        ];
        let title = fit_text(&item.title, 300, 24.5, &self.assets.bold);
        draw_text(
            image,
            (
                x + 142,
                middle_origin_y(y + 44, &title, 24.5, &self.assets.bold),
            ),
            &title,
            24.5,
            text_colors[difficulty],
            &self.assets.bold,
        );
        let id = format!("ID: {}", id_text(&item.display_id));
        draw_text(
            image,
            (
                x + 145,
                middle_origin_y(y + 124, &id, 21.25, &self.assets.regular),
            ),
            &id,
            21.25,
            text_colors[difficulty],
            &self.assets.regular,
        );
        let old = item
            .old_achievement
            .map_or_else(|| "0.0000%".to_owned(), format_achievement);
        draw_centered(
            image,
            x + 210,
            y + 71,
            &old,
            29.25,
            text_colors[difficulty],
            &self.assets.regular,
        );
        draw_centered(
            image,
            x + 370,
            y + 71,
            &format_achievement(item.target_achievement),
            29.25,
            text_colors[difficulty],
            &self.assets.regular,
        );
        draw_centered(
            image,
            x + 245,
            y + 96,
            &format!("Ra: {}", item.old_rating),
            20.0,
            text_colors[difficulty],
            &self.assets.regular,
        );
        draw_centered(
            image,
            x + 415,
            y + 96,
            &format!("Ra: {}", item.target_rating),
            20.0,
            colors[difficulty],
            &self.assets.regular,
        );
        let constant = format!("ds:{}", format_constant(item.constant));
        draw_text(
            image,
            (
                x + 315,
                middle_origin_y(y + 124, &constant, 21.25, &self.assets.regular),
            ),
            &constant,
            21.25,
            colors[difficulty],
            &self.assets.regular,
        );
        let gain = format!("Ra +{}", item.gain);
        draw_text(
            image,
            (
                x + 390,
                middle_origin_y(y + 124, &gain, 21.25, &self.assets.regular),
            ),
            &gain,
            21.25,
            colors[difficulty],
            &self.assets.regular,
        );
    }
}

fn draw_rank(image: &mut RgbaImage, rank: &RgbaImage, (x, y): (i32, i32)) {
    overlay(
        image,
        &resize(rank, 63, 28, FilterType::Lanczos3),
        i64::from(x),
        i64::from(y),
    );
}

fn id_text(value: &SongIdValue) -> String {
    match value {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}

fn difficulty_index(value: Difficulty) -> usize {
    match value {
        Difficulty::Basic => 0,
        Difficulty::Advanced => 1,
        Difficulty::Expert => 2,
        Difficulty::Master => 3,
        Difficulty::ReMaster | Difficulty::Utage => 4,
    }
}

fn rank_index(value: AchievementRank) -> usize {
    match value {
        AchievementRank::D => 0,
        AchievementRank::C => 1,
        AchievementRank::B => 2,
        AchievementRank::Bb => 3,
        AchievementRank::Bbb => 4,
        AchievementRank::A => 5,
        AchievementRank::Aa => 6,
        AchievementRank::Aaa => 7,
        AchievementRank::S => 8,
        AchievementRank::SPlus => 9,
        AchievementRank::Ss => 10,
        AchievementRank::SsPlus => 11,
        AchievementRank::Sss => 12,
        AchievementRank::SssPlus => 13,
    }
}
