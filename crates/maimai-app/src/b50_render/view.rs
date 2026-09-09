use maimai_core::{ChartGeneration, Difficulty};
use maimai_render::{B50View, ChartType, Difficulty as RenderDifficulty, PlayerHeader, ScoreCard};

use crate::scores::{B50Chart, B50Result, RatingMode};

use super::B50RenderError;

pub(super) fn prepare(
    result: &B50Result,
    title: Option<String>,
) -> Result<B50View, B50RenderError> {
    let nickname = result
        .player
        .nickname
        .as_deref()
        .ok_or(B50RenderError::PlayerNotFound)?;
    let rating = if result.mode == RatingMode::Fit {
        Some(result.rating_breakdown.total)
    } else {
        result.player.rating.or(result.player.actual_rating)
    };
    let header = PlayerHeader::new(nickname, rating, result.player.plate.clone())
        .map_err(|error| B50RenderError::invalid(error.to_string()))?;
    let b35 = cards(&result.b35)?;
    let b15 = cards(&result.b15)?;
    B50View::new(
        title.unwrap_or_else(|| "maimai DX Best 50".to_owned()),
        header,
        result.rating_breakdown,
        b35,
        b15,
    )
    .map_err(|error| B50RenderError::invalid(error.to_string()))
}

fn cards(charts: &[B50Chart]) -> Result<Vec<ScoreCard>, B50RenderError> {
    charts.iter().map(card).collect()
}

fn card(chart: &B50Chart) -> Result<ScoreCard, B50RenderError> {
    let chart_type = match chart.key.generation() {
        ChartGeneration::Standard => ChartType::Standard,
        ChartGeneration::Deluxe => ChartType::Deluxe,
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer => {
            return Err(B50RenderError::invalid("B50 绘图不支持宴谱。"));
        }
    };
    let difficulty = match chart.key.difficulty() {
        Difficulty::Basic => RenderDifficulty::Basic,
        Difficulty::Advanced => RenderDifficulty::Advanced,
        Difficulty::Expert => RenderDifficulty::Expert,
        Difficulty::Master => RenderDifficulty::Master,
        Difficulty::ReMaster => RenderDifficulty::ReMaster,
        Difficulty::Utage => return Err(B50RenderError::invalid("B50 绘图不支持宴谱。")),
    };
    let constant = if chart.original_rating.is_some() {
        chart.fit_constant.or(chart.constant)
    } else {
        chart.constant
    };
    ScoreCard::new(
        Some(chart.source_song_id.clone()),
        chart.title.clone(),
        chart_type,
        difficulty,
        chart.level.clone(),
        constant,
        chart.achievements.and_then(|value| value.ranked()),
        chart.rating.unwrap_or_default(),
    )
    .and_then(|card| {
        card.with_markers(
            chart.grade.clone(),
            chart.full_combo.map(|value| value.as_str().to_owned()),
            chart.full_sync.map(|value| value.as_str().to_owned()),
        )
    })
    .map_err(|error| B50RenderError::invalid(error.to_string()))
}
