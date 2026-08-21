use image::{
    Rgba, RgbaImage,
    imageops::{FilterType, overlay, resize},
};
use imageproc::drawing::text_size;
use maimai_core::{
    AchievementRank, ChartGeneration, Difficulty, FullComboStatus, FullSyncStatus, SongIdValue,
    achievement_rank,
};

use super::{ScoreListRenderError, assets::ScoreListAssets, model::ScoreListItem};
use crate::{
    CoverResolver,
    cover::ResolvedCover,
    text::{draw_text, format_constant, format_play_achievement, middle_origin_y},
};

pub(crate) struct ScoreCardDrawer<'a> {
    assets: &'a ScoreListAssets,
    covers: &'a CoverResolver,
}

impl<'a> ScoreCardDrawer<'a> {
    pub(crate) const fn new(assets: &'a ScoreListAssets, covers: &'a CoverResolver) -> Self {
        Self { assets, covers }
    }

    pub(crate) fn draw(
        &self,
        image: &mut RgbaImage,
        (x, y): (i32, i32),
        item: &ScoreListItem,
    ) -> Result<bool, ScoreListRenderError> {
        let difficulty = difficulty_index(item.difficulty);
        overlay(
            image,
            &self.assets.cards[difficulty],
            i64::from(x),
            i64::from(y),
        );
        let (cover, placeholder) = match self
            .covers
            .resolve_music(Some(&item.cover_id), item.image_name.as_deref())?
        {
            ResolvedCover::Image(image) => (image, false),
            ResolvedCover::Missing(_) => (
                RgbaImage::from_pixel(75, 75, Rgba([224, 229, 245, 255])),
                true,
            ),
        };
        overlay(
            image,
            &resize(&cover, 75, 75, FilterType::Lanczos3),
            i64::from(x + 12),
            i64::from(y + 12),
        );
        self.draw_icons(image, (x, y), item);
        self.draw_text(image, (x, y), item, difficulty);
        Ok(placeholder)
    }

    fn draw_icons(&self, image: &mut RgbaImage, (x, y): (i32, i32), item: &ScoreListItem) {
        let generation = if item.generation == ChartGeneration::Standard {
            &self.assets.standard
        } else {
            &self.assets.deluxe
        };
        overlay(
            image,
            &resize(generation, 37, 14, FilterType::Lanczos3),
            i64::from(x + 51),
            i64::from(y + 91),
        );
        if let Some(achievement) = item.achievement.and_then(|value| value.ranked()) {
            draw_icon(
                image,
                &self.assets.ranks[rank_index(achievement_rank(achievement))],
                (63, 28),
                (x + 92, y + 78),
            );
        }
        if let Some(combo) = item.combo {
            draw_icon(
                image,
                &self.assets.combo[combo_index(combo)],
                (34, 34),
                (x + 154, y + 77),
            );
        }
        if let Some(sync) = item.sync {
            draw_icon(
                image,
                &self.assets.sync[sync_index(sync)],
                (34, 34),
                (x + 185, y + 77),
            );
        }
        if let Some(stars) = dx_stars(item.dx_score, item.max_dx_score) {
            draw_icon(
                image,
                &self.assets.dx_gauges[stars - 1],
                (47, 26),
                (x + 217, y + 80),
            );
        }
    }

    fn draw_text(
        &self,
        image: &mut RgbaImage,
        (x, y): (i32, i32),
        item: &ScoreListItem,
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
        draw_centered_at(
            image,
            (x + 26, y + 98),
            &id_text(&item.display_id),
            16.5,
            colors[difficulty],
            &self.assets.regular,
        );
        let title = legacy_title(&item.title);
        draw_middle_left(
            image,
            (x + 93, y + 14),
            &title,
            20.0,
            text_colors[difficulty],
            &self.assets.bold,
        );
        let achievement = item
            .achievement
            .map_or_else(|| "-.----%".to_owned(), format_play_achievement);
        draw_middle_left(
            image,
            (x + 93, y + 38),
            &achievement,
            36.0,
            text_colors[difficulty],
            &self.assets.regular,
        );
        let constant = item
            .constant
            .map_or_else(|| "-".to_owned(), format_constant);
        let rating = item
            .rating
            .map_or_else(|| "-".to_owned(), |value| value.to_string());
        draw_middle_left(
            image,
            (x + 93, y + 65),
            &format!("{constant} -> {rating}"),
            18.0,
            text_colors[difficulty],
            &self.assets.regular,
        );
        let dx = item
            .dx_score
            .map_or_else(|| "-".to_owned(), |value| value.to_string());
        let max = item
            .max_dx_score
            .map_or_else(|| "-".to_owned(), |value| value.to_string());
        draw_centered_at(
            image,
            (x + 219, y + 65),
            &format!("{dx}/{max}"),
            18.0,
            text_colors[difficulty],
            &self.assets.regular,
        );
    }
}

fn draw_middle_left(
    image: &mut RgbaImage,
    center: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &ab_glyph::FontArc,
) {
    draw_text(
        image,
        (center.0, middle_origin_y(center.1, text, size, font)),
        text,
        size,
        color,
        font,
    );
}

fn draw_centered_at(
    image: &mut RgbaImage,
    center: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &ab_glyph::FontArc,
) {
    let (width, _) = text_size(size, font, text);
    draw_text(
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

fn draw_icon(image: &mut RgbaImage, icon: &RgbaImage, size: (u32, u32), xy: (i32, i32)) {
    overlay(
        image,
        &resize(icon, size.0, size.1, FilterType::Lanczos3),
        i64::from(xy.0),
        i64::from(xy.1),
    );
}

fn id_text(value: &SongIdValue) -> String {
    match value {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}

fn legacy_title(value: &str) -> String {
    if display_columns(value) <= 18 {
        return value.to_owned();
    }
    let mut width = 0_usize;
    let mut result = String::new();
    for character in value.chars() {
        let next = width + usize::from(!character.is_ascii()) + 1;
        if next > 17 {
            break;
        }
        width = next;
        result.push(character);
    }
    result.push_str("...");
    result
}

fn display_columns(value: &str) -> usize {
    value
        .chars()
        .map(|character| usize::from(!character.is_ascii()) + 1)
        .sum()
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

fn combo_index(value: FullComboStatus) -> usize {
    match value {
        FullComboStatus::FullCombo => 0,
        FullComboStatus::FullComboPlus => 1,
        FullComboStatus::AllPerfect => 2,
        FullComboStatus::AllPerfectPlus => 3,
    }
}

fn sync_index(value: FullSyncStatus) -> usize {
    match value {
        FullSyncStatus::Sync => 0,
        FullSyncStatus::FullSync => 1,
        FullSyncStatus::FullSyncPlus => 2,
        FullSyncStatus::FullSyncDeluxe => 3,
        FullSyncStatus::FullSyncDeluxePlus => 4,
    }
}

fn dx_stars(score: Option<u32>, maximum: Option<u32>) -> Option<usize> {
    let (score, maximum) = (score?, maximum?);
    crate::deluxe_score::star_level(u64::from(score), u64::from(maximum))
        .filter(|level| *level > 0)
        .map(usize::from)
}
