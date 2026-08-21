use image::{
    Rgba, RgbaImage,
    imageops::{FilterType, overlay, resize},
};
use imageproc::drawing::text_size;

use super::{
    CompletionRenderer, encode,
    model::{CompletionRenderedPng, RatingTableMode, RatingTableView},
};
use crate::{
    RenderError,
    text::{draw_text, middle_origin_y},
};

#[cfg(test)]
const RATING_TABLE_WIDTH: u32 = 1_400;
const ACCENT: Rgba<u8> = Rgba([124, 130, 255, 255]);

impl CompletionRenderer {
    pub fn render_rating_table(
        &self,
        view: &RatingTableView,
    ) -> Result<CompletionRenderedPng, RenderError> {
        view.validate()?;
        let image = self.assets.rating_template(&view.level)?.ok_or_else(|| {
            RenderError::invalid("rating.level", "missing original rating template")
        })?;
        self.render_legacy_rating_table(view, image)
    }

    fn render_legacy_rating_table(
        &self,
        view: &RatingTableView,
        mut image: RgbaImage,
    ) -> Result<CompletionRenderedPng, RenderError> {
        overlay(&mut image, &self.assets.pic("rating_bg.png")?, 600, 25);
        centered_stroked_text(
            &mut image,
            (305, 60),
            &format!("Level.{}", view.level),
            96.0,
            ACCENT,
            &self.assets.bold,
            5,
        );
        centered_stroked_text(
            &mut image,
            (305, 130),
            "定数表",
            96.0,
            ACCENT,
            &self.assets.bold,
            5,
        );
        centered_stroked_text(
            &mut image,
            (700, 127),
            &view.total.to_string(),
            72.0,
            ACCENT,
            &self.assets.regular,
            5,
        );
        for (index, value) in view.statistics.values().into_iter().enumerate() {
            let x = 824 + (index % 8) as i32 * 64;
            let y = 78 + (index / 8) as i32 * 56;
            centered_stroked_text(
                &mut image,
                (x, y),
                &value.to_string(),
                29.0,
                ACCENT,
                &self.assets.regular,
                2,
            );
        }

        let complete = self.assets.pic("complete_bg.png")?;
        let unfinished = self.assets.pic("unfinished_bg.png")?;
        let mut y = 118_i32;
        for group in &view.groups {
            y += 20;
            for (index, cell) in group.cells.iter().enumerate() {
                let x = 158 + (index % 14) as i32 * 85;
                if index % 14 == 0 {
                    y += 85;
                }
                match view.mode {
                    RatingTableMode::Achievement => {
                        let Some(achievement) = cell.achievement else {
                            continue;
                        };
                        let state = if achievement.ten_thousandths() >= 1_000_000 {
                            &complete
                        } else {
                            &unfinished
                        };
                        overlay(&mut image, state, i64::from(x + 2), i64::from(y - 18));
                        if let Some(rank) =
                            self.assets.optional_pic(achievement_asset(achievement))?
                        {
                            overlay(
                                &mut image,
                                &resize(&rank, 78, 35, FilterType::Lanczos3),
                                i64::from(x),
                                i64::from(y - 5),
                            );
                        }
                    }
                    RatingTableMode::FullCombo => {
                        let Some(combo) = cell.combo else {
                            continue;
                        };
                        overlay(&mut image, &complete, i64::from(x + 2), i64::from(y - 18));
                        if let Some(icon) = self.assets.optional_pic(combo_asset(combo))? {
                            overlay(
                                &mut image,
                                &resize(&icon, 50, 50, FilterType::Lanczos3),
                                i64::from(x + 15),
                                i64::from(y - 12),
                            );
                        }
                    }
                }
            }
        }
        if let Some(all_clear) = view.all_clear
            && let Some(icon) = self.assets.optional_pic(all_clear_asset(all_clear))?
        {
            overlay(&mut image, &icon, 40, 40);
        }
        let (bytes, width, height) = encode(image)?;
        Ok(CompletionRenderedPng {
            bytes,
            width,
            height,
            placeholder_covers: 0,
        })
    }
}

fn achievement_asset(value: maimai_core::AchievementRate) -> &'static str {
    use maimai_core::AchievementRank;
    match maimai_core::achievement_rank(value) {
        AchievementRank::D => "UI_TTR_Rank_D.png",
        AchievementRank::C => "UI_TTR_Rank_C.png",
        AchievementRank::B => "UI_TTR_Rank_B.png",
        AchievementRank::Bb => "UI_TTR_Rank_BB.png",
        AchievementRank::Bbb => "UI_TTR_Rank_BBB.png",
        AchievementRank::A => "UI_TTR_Rank_A.png",
        AchievementRank::Aa => "UI_TTR_Rank_AA.png",
        AchievementRank::Aaa => "UI_TTR_Rank_AAA.png",
        AchievementRank::S => "UI_TTR_Rank_S.png",
        AchievementRank::SPlus => "UI_TTR_Rank_Sp.png",
        AchievementRank::Ss => "UI_TTR_Rank_SS.png",
        AchievementRank::SsPlus => "UI_TTR_Rank_SSp.png",
        AchievementRank::Sss => "UI_TTR_Rank_SSS.png",
        AchievementRank::SssPlus => "UI_TTR_Rank_SSSp.png",
    }
}

const fn combo_asset(value: maimai_core::FullComboStatus) -> &'static str {
    match value {
        maimai_core::FullComboStatus::FullCombo => "UI_MSS_MBase_Icon_FC.png",
        maimai_core::FullComboStatus::FullComboPlus => "UI_MSS_MBase_Icon_FCp.png",
        maimai_core::FullComboStatus::AllPerfect => "UI_MSS_MBase_Icon_AP.png",
        maimai_core::FullComboStatus::AllPerfectPlus => "UI_MSS_MBase_Icon_APp.png",
    }
}

const fn all_clear_asset(value: super::model::RatingAllClear) -> &'static str {
    use maimai_core::{AchievementRank, FullComboStatus};
    match value {
        super::model::RatingAllClear::Achievement(rank) => match rank {
            AchievementRank::S => "UI_MSS_Allclear_Icon_S.png",
            AchievementRank::SPlus => "UI_MSS_Allclear_Icon_Sp.png",
            AchievementRank::Ss => "UI_MSS_Allclear_Icon_SS.png",
            AchievementRank::SsPlus => "UI_MSS_Allclear_Icon_SSp.png",
            AchievementRank::Sss => "UI_MSS_Allclear_Icon_SSS.png",
            AchievementRank::SssPlus => "UI_MSS_Allclear_Icon_SSSp.png",
            _ => "UI_MSS_Allclear_Icon_S.png",
        },
        super::model::RatingAllClear::FullCombo(combo) => match combo {
            FullComboStatus::FullCombo => "UI_MSS_Allclear_Icon_FC.png",
            FullComboStatus::FullComboPlus => "UI_MSS_Allclear_Icon_FCp.png",
            FullComboStatus::AllPerfect => "UI_MSS_Allclear_Icon_AP.png",
            FullComboStatus::AllPerfectPlus => "UI_MSS_Allclear_Icon_APp.png",
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn centered_stroked_text(
    image: &mut RgbaImage,
    center: (i32, i32),
    text: &str,
    size: f32,
    color: Rgba<u8>,
    font: &ab_glyph::FontArc,
    stroke: i32,
) {
    let (width, _) = text_size(size, font, text);
    let origin = (
        center.0 - i32::try_from(width).unwrap_or(i32::MAX) / 2,
        middle_origin_y(center.1, text, size, font),
    );
    for dy in -stroke..=stroke {
        for dx in -stroke..=stroke {
            if dx != 0 || dy != 0 {
                draw_text(
                    image,
                    (origin.0 + dx, origin.1 + dy),
                    text,
                    size,
                    Rgba([255, 255, 255, 255]),
                    font,
                );
            }
        }
    }
    draw_text(image, origin, text, size, color, font);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::{GenericImageView, ImageReader};
    use maimai_core::{
        AchievementRate, ChartConstant, ChartGeneration, Difficulty, FullComboStatus, SongIdValue,
    };
    use tempfile::TempDir;

    use super::RATING_TABLE_WIDTH;
    use crate::completion::{
        CompletionRenderer, RatingConstantGroup, RatingScoreCell, RatingStatistics,
        RatingTableMode, RatingTableView,
    };

    fn cell(generation: ChartGeneration) -> Result<RatingScoreCell, Box<dyn std::error::Error>> {
        Ok(RatingScoreCell {
            cover_id: SongIdValue::Numeric(999_999),
            image_name: None,
            title: "missing fixture cover".to_owned(),
            generation,
            difficulty: Difficulty::Master,
            achievement: Some(AchievementRate::from_decimal_str("100.5")?),
            combo: Some(FullComboStatus::AllPerfectPlus),
        })
    }

    fn group(count: usize) -> Result<RatingConstantGroup, Box<dyn std::error::Error>> {
        let mut cells = Vec::new();
        for index in 0..count {
            cells.push(cell(if index % 2 == 0 {
                ChartGeneration::Standard
            } else {
                ChartGeneration::Deluxe
            })?);
        }
        Ok(RatingConstantGroup {
            constant: Some(ChartConstant::from_decimal_str("15.0")?),
            cells,
        })
    }

    #[test]
    fn achievement_and_fc_modes_render_on_the_legacy_template()
    -> Result<(), Box<dyn std::error::Error>> {
        let cache = TempDir::new()?;
        let renderer = CompletionRenderer::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../maimaidx_render_mcp/static"),
            cache.path(),
        )?;
        let group = group(2)?;
        let view = |mode| RatingTableView {
            level: "15".to_owned(),
            mode,
            total: 2,
            statistics: RatingStatistics::default(),
            all_clear: None,
            groups: vec![group.clone()],
        };
        let achievement = renderer.render_rating_table(&view(RatingTableMode::Achievement))?;
        let combo = renderer.render_rating_table(&view(RatingTableMode::FullCombo))?;
        assert_eq!(
            (achievement.width, achievement.height),
            (RATING_TABLE_WIDTH, 570)
        );
        assert_eq!(achievement.placeholder_covers, 0);
        let achievement_image = ImageReader::new(std::io::Cursor::new(&achievement.bytes))
            .with_guessed_format()?
            .decode()?;
        let combo_image = ImageReader::new(std::io::Cursor::new(&combo.bytes))
            .with_guessed_format()?
            .decode()?;
        assert_eq!(achievement_image.dimensions(), (RATING_TABLE_WIDTH, 570));
        assert!((160..235).any(|x| {
            (205..273).any(|y| achievement_image.get_pixel(x, y) != combo_image.get_pixel(x, y))
        }));
        Ok(())
    }
}
