use std::str::FromStr;

use maimai_app::scores::{B50Chart, FitLabel};
use maimai_core::{ChartGeneration, Difficulty, SongIdValue};
use serde_json::{Number, Value, json};

use crate::score_queries::error::ScoreQueryToolError;

pub(super) fn charts(
    values: &[B50Chart],
    include_metadata: bool,
) -> Result<Value, ScoreQueryToolError> {
    values
        .iter()
        .map(|value| chart(value, include_metadata))
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn chart(chart: &B50Chart, include_metadata: bool) -> Result<Value, ScoreQueryToolError> {
    let mut output = json!({
        "title":chart.title,"type":generation(chart.key.generation()),"level":chart.level,
        "levelLabel":difficulty_label(chart.key.difficulty()),
        "levelIndex":difficulty_index(chart.key.difficulty()),"ds":optional_constant(chart.constant)?,
        "achievements":optional_achievement(chart.achievements)?,"dxScore":chart.dx_score,
        "fc":chart.full_combo,"fs":chart.full_sync,"ra":chart.rating,"rate":chart.grade,
        "songId":song_id(chart.source_song_id.value()),"version":chart.version,"isNew":chart.is_current,
    });
    if chart.original_rating.is_some() {
        output["fittedRa"] = json!(chart.rating);
        output["originalRa"] = json!(chart.original_rating);
        output["ratingBase"] = json!("fitDiff");
    }
    if include_metadata {
        output["fitDiff"] = optional_constant(chart.fit_constant)?;
        output["fitDelta"] = chart
            .constant
            .zip(chart.fit_constant)
            .map(|(actual, fit)| number((actual.value() - fit.value()).to_string()))
            .transpose()?
            .map_or(Value::Null, Value::Number);
        output["fitLabel"] = json!(chart.fit_label.map(fit_label));
    }
    Ok(output)
}

fn optional_constant(
    value: Option<maimai_core::ChartConstant>,
) -> Result<Value, ScoreQueryToolError> {
    value
        .map(|value| number(value.value().to_string()))
        .transpose()
        .map(|value| value.map_or(Value::Null, Value::Number))
}

fn optional_achievement(
    value: Option<maimai_core::PlayAchievement>,
) -> Result<Value, ScoreQueryToolError> {
    value
        .map(|value| number(value.decimal_string()))
        .transpose()
        .map(|value| value.map_or(Value::Null, Value::Number))
}

fn number(value: String) -> Result<Number, ScoreQueryToolError> {
    Number::from_str(&value).map_err(|_| ScoreQueryToolError::internal())
}

fn song_id(value: &SongIdValue) -> Value {
    match value {
        SongIdValue::Numeric(value) => json!(value),
        SongIdValue::Text(value) => json!(value),
    }
}
fn generation(value: ChartGeneration) -> &'static str {
    match value {
        ChartGeneration::Standard => "SD",
        _ => "DX",
    }
}
fn difficulty_label(value: Difficulty) -> &'static str {
    match value {
        Difficulty::Basic => "Basic",
        Difficulty::Advanced => "Advanced",
        Difficulty::Expert => "Expert",
        Difficulty::Master => "Master",
        Difficulty::ReMaster => "Re:MASTER",
        Difficulty::Utage => "Utage",
    }
}
fn difficulty_index(value: Difficulty) -> Option<u8> {
    match value {
        Difficulty::Basic => Some(0),
        Difficulty::Advanced => Some(1),
        Difficulty::Expert => Some(2),
        Difficulty::Master => Some(3),
        Difficulty::ReMaster => Some(4),
        Difficulty::Utage => None,
    }
}
fn fit_label(value: FitLabel) -> &'static str {
    match value {
        FitLabel::Inflated => "虚高",
        FitLabel::Deflated => "虚低",
        FitLabel::Equal => "持平",
    }
}
