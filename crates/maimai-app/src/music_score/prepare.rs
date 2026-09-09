use std::collections::HashMap;

use maimai_core::{AchievementRank, ChartGeneration, achievement_rank};
use maimai_render::{MusicScoreRow, MusicScoreView};

use crate::{music_info::resolve::PreparedMusicInfo, scores::B50Chart};

use super::MusicScoreError;

pub(super) fn view(
    prepared: &PreparedMusicInfo,
    best: &HashMap<maimai_core::ChartKey, &B50Chart>,
) -> Result<MusicScoreView, MusicScoreError> {
    let mut rows = Vec::new();
    for chart in &prepared.view.charts {
        let key = prepared
            .chart_keys
            .iter()
            .find(|key| key.difficulty() == chart.difficulty);
        let record = key.and_then(|key| best.get(key).copied());
        let note_total = chart
            .note_total
            .map(u64::from)
            .unwrap_or_else(|| chart.notes.map_or(0, |notes| notes.total()));
        rows.push(MusicScoreRow::new(
            chart.difficulty,
            chart.level.clone(),
            chart
                .constant
                .or_else(|| record.and_then(|value| value.constant)),
            note_total,
            record.is_some(),
            record.and_then(|value| value.achievements),
            record.and_then(grade),
            record.and_then(|value| value.full_combo),
            record.and_then(|value| value.full_sync),
            record.and_then(|value| value.dx_score),
            record.and_then(|value| value.rating),
        )?);
    }
    Ok(MusicScoreView::new(
        prepared.view.clone(),
        prepared.generation,
        rows,
    )?)
}

pub(super) const fn chart_type(generation: ChartGeneration) -> &'static str {
    match generation {
        ChartGeneration::Standard => "ST",
        ChartGeneration::Deluxe => "DX",
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer => "UTAGE",
    }
}

fn grade(record: &B50Chart) -> Option<String> {
    record
        .grade
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(display_grade)
        .or_else(|| {
            record
                .achievements
                .and_then(|value| value.ranked())
                .map(achievement_rank)
                .map(rank_label)
        })
        .map(str::to_owned)
}

fn display_grade(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "sssp" | "sss+" => "SSS+",
        "sss" => "SSS",
        "ssp" | "ss+" => "SS+",
        "ss" => "SS",
        "sp" | "s+" => "S+",
        "s" => "S",
        "aaa" => "AAA",
        "aa" => "AA",
        "a" => "A",
        "bbb" => "BBB",
        "bb" => "BB",
        "b" => "B",
        "c" => "C",
        "d" => "D",
        _ => value,
    }
}

const fn rank_label(value: AchievementRank) -> &'static str {
    match value {
        AchievementRank::SssPlus => "SSS+",
        AchievementRank::Sss => "SSS",
        AchievementRank::SsPlus => "SS+",
        AchievementRank::Ss => "SS",
        AchievementRank::SPlus => "S+",
        AchievementRank::S => "S",
        AchievementRank::Aaa => "AAA",
        AchievementRank::Aa => "AA",
        AchievementRank::A => "A",
        AchievementRank::Bbb => "BBB",
        AchievementRank::Bb => "BB",
        AchievementRank::B => "B",
        AchievementRank::C => "C",
        AchievementRank::D => "D",
    }
}
