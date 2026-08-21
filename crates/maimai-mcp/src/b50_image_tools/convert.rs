use std::str::FromStr;

use maimai_core::{SongIdNamespace, SongIdValue};
use maimai_render::{
    AchievementRate, B50View, ChartConstant, ChartType, Difficulty, PlayerHeader, RatingBreakdown,
    ScoreCard, SourceSongId,
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::{
    dto::{B50DataDto, ChartDto, ExactNumber, SongIdDto},
    error::B50ToolError,
};

pub(super) fn b50_view(data: &B50DataDto, title: Option<String>) -> Result<B50View, B50ToolError> {
    let title = display_text(title, "maimai DX Best 50", "title")?;
    let player = data.player.as_ref();
    let nickname = display_text(
        player.and_then(|value| value.nickname.clone()),
        "未知玩家",
        "b50Data.player.nickname",
    )?;
    let plate = optional_text(
        player.and_then(|value| value.plate.clone()),
        "b50Data.player.plate",
    )?;
    let rating = player
        .and_then(|value| value.rating)
        .filter(|value| *value != 0);
    let player = PlayerHeader::new(nickname, rating, plate)
        .map_err(|error| B50ToolError::invalid(error.to_string()))?;
    let breakdown = data.rating_breakdown.map_or(
        RatingBreakdown {
            b35: 0,
            b15: 0,
            total: 0,
        },
        |value| RatingBreakdown {
            b35: value.sd,
            b15: value.dx,
            total: value.total,
        },
    );
    let b35 = data
        .charts
        .sd
        .iter()
        .enumerate()
        .map(|(index, chart)| score_card(chart, ChartType::Standard, "sd", index))
        .collect::<Result<Vec<_>, _>>()?;
    let b15 = data
        .charts
        .dx
        .iter()
        .enumerate()
        .map(|(index, chart)| score_card(chart, ChartType::Deluxe, "dx", index))
        .collect::<Result<Vec<_>, _>>()?;
    B50View::new(title, player, breakdown, b35, b15)
        .map_err(|error| B50ToolError::invalid(error.to_string()))
}

pub(super) fn filename_stem(data: &B50DataDto) -> String {
    data.lookup
        .as_ref()
        .and_then(|lookup| lookup.qq.as_deref().or(lookup.username.as_deref()))
        .or_else(|| {
            data.player
                .as_ref()
                .and_then(|player| player.nickname.as_deref())
        })
        .unwrap_or("b50")
        .to_owned()
}

fn score_card(
    chart: &ChartDto,
    expected_type: ChartType,
    section: &str,
    index: usize,
) -> Result<ScoreCard, B50ToolError> {
    let field = format!("b50Data.charts.{section}[{index}]");
    let chart_type = parse_chart_type(chart.chart_type.as_deref(), expected_type, &field)?;
    let difficulty = parse_difficulty(chart, &field)?;
    let title = display_text(chart.title.clone(), "未知曲目", &format!("{field}.title"))?;
    let level = display_text(chart.level.clone(), "-", &format!("{field}.level"))?;
    let constant = chart
        .ds
        .as_ref()
        .map(|value| parse_constant(value, &format!("{field}.ds")))
        .transpose()?;
    let achievements = chart
        .achievements
        .as_ref()
        .map(|value| parse_achievement(value, &format!("{field}.achievements")))
        .transpose()?;
    let card = ScoreCard::new(
        chart.song_id.as_ref().map(source_song_id).transpose()?,
        title,
        chart_type,
        difficulty,
        level,
        constant,
        achievements,
        chart.ra,
    )
    .map_err(|error| B50ToolError::invalid(error.to_string()))?;
    card.with_markers(
        optional_text(chart.rate.clone(), &format!("{field}.rate"))?,
        optional_text(chart.fc.clone(), &format!("{field}.fc"))?,
        optional_text(chart.fs.clone(), &format!("{field}.fs"))?,
    )
    .map_err(|error| B50ToolError::invalid(error.to_string()))
}

fn parse_chart_type(
    value: Option<&str>,
    expected: ChartType,
    field: &str,
) -> Result<ChartType, B50ToolError> {
    let parsed = match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => expected,
        Some(value)
            if matches!(
                value.to_ascii_lowercase().as_str(),
                "sd" | "st" | "standard"
            ) =>
        {
            ChartType::Standard
        }
        Some(value) if matches!(value.to_ascii_lowercase().as_str(), "dx" | "deluxe") => {
            ChartType::Deluxe
        }
        Some(_) => return Err(B50ToolError::invalid(format!("{field}.type 无效。"))),
    };
    if parsed != expected {
        return Err(B50ToolError::invalid(format!(
            "{field}.type 与所在 B50 分区不一致。"
        )));
    }
    Ok(parsed)
}

fn parse_difficulty(chart: &ChartDto, field: &str) -> Result<Difficulty, B50ToolError> {
    if let Some(label) = chart
        .level_label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return match label.to_ascii_lowercase().as_str() {
            "basic" => Ok(Difficulty::Basic),
            "advanced" => Ok(Difficulty::Advanced),
            "expert" => Ok(Difficulty::Expert),
            "master" => Ok(Difficulty::Master),
            "re:master" | "remaster" | "re-master" => Ok(Difficulty::ReMaster),
            _ => Err(B50ToolError::invalid(format!("{field}.levelLabel 无效。"))),
        };
    }
    match chart.level_index.unwrap_or(3) {
        0 => Ok(Difficulty::Basic),
        1 => Ok(Difficulty::Advanced),
        2 => Ok(Difficulty::Expert),
        3 => Ok(Difficulty::Master),
        4 => Ok(Difficulty::ReMaster),
        _ => Err(B50ToolError::invalid(format!(
            "{field}.levelIndex 必须是 0 到 4。"
        ))),
    }
}

fn parse_constant(value: &ExactNumber, field: &str) -> Result<ChartConstant, B50ToolError> {
    ChartConstant::from_decimal_str(&value.text())
        .map_err(|_| B50ToolError::invalid(format!("{field} 不是有效定数。")))
}

fn parse_achievement(value: &ExactNumber, field: &str) -> Result<AchievementRate, B50ToolError> {
    AchievementRate::from_decimal_str(&value.text())
        .map_err(|_| B50ToolError::invalid(format!("{field} 不是有效达成率。")))
}

fn source_song_id(value: &SongIdDto) -> Result<SourceSongId, B50ToolError> {
    let value = match value {
        SongIdDto::Number(number) => {
            let text = number.to_string();
            let decimal = Decimal::from_str(&text)
                .or_else(|_| Decimal::from_scientific(&text))
                .map_err(|_| B50ToolError::invalid("songId 必须是整数或非空字符串。"))?;
            let numeric = decimal
                .fract()
                .is_zero()
                .then(|| decimal.to_u32())
                .flatten()
                .ok_or_else(|| B50ToolError::invalid("songId 数字超出范围。"))?;
            SongIdValue::Numeric(numeric)
        }
        SongIdDto::Text(text) => match text.trim().parse::<u32>() {
            Ok(numeric) => SongIdValue::Numeric(numeric),
            Err(_) => SongIdValue::text(text.clone())
                .map_err(|_| B50ToolError::invalid("songId 必须是整数或非空字符串。"))?,
        },
    };
    Ok(SourceSongId::new(SongIdNamespace::DivingFish, value))
}

fn display_text(
    value: Option<String>,
    fallback: &str,
    field: &str,
) -> Result<String, B50ToolError> {
    let Some(value) = value else {
        return Ok(fallback.to_owned());
    };
    if value.chars().any(char::is_control) {
        return Err(B50ToolError::invalid(format!("{field} 不能包含控制字符。")));
    }
    let value = value.trim();
    Ok(if value.is_empty() { fallback } else { value }.to_owned())
}

fn optional_text(value: Option<String>, field: &str) -> Result<Option<String>, B50ToolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.chars().any(char::is_control) {
        return Err(B50ToolError::invalid(format!("{field} 不能包含控制字符。")));
    }
    let value = value.trim();
    Ok((!value.is_empty()).then(|| value.to_owned()))
}
