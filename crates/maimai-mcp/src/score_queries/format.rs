use maimai_app::scores::{
    B50Chart, B50Result, ExactRatio, FitIndexLabel, FitIndexSection, FitLabel, Lookup, RatingMode,
};
use maimai_storage::IdentityRecord;
use rust_decimal::{Decimal, RoundingStrategy};
use serde_json::Value;

use super::{
    error::ScoreQueryToolError,
    filter::{DisplayOptions, Section},
};

pub fn b50(
    result: &B50Result,
    identity: Option<&IdentityRecord>,
    options: &DisplayOptions,
) -> Result<String, ScoreQueryToolError> {
    let mut lines = vec![format!(
        "昵称: {}",
        result.player.nickname.as_deref().unwrap_or("未知")
    )];
    append_identity(&mut lines, &result.lookup, identity);
    lines.extend([
        format!(
            "Rating: {}",
            result
                .player
                .rating
                .map_or_else(|| "未知".to_owned(), |value| value.to_string())
        ),
        format!("牌子: {}", result.player.plate.as_deref().unwrap_or("未知")),
        format!(
            "旧曲 Best: {} 首，合计 ra {}",
            result.b35.len(),
            result.rating_breakdown.b35
        ),
        format!(
            "新曲 Best: {} 首，合计 ra {}",
            result.b15.len(),
            result.rating_breakdown.b15
        ),
        format!(
            "B50 合计: {} 首，合计 ra {}",
            result.total_count(),
            result.rating_breakdown.total
        ),
    ]);
    if result.mode == RatingMode::Fit {
        let mut versions = result
            .b15
            .iter()
            .map(|chart| chart.version.as_str())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        versions.sort_unstable();
        versions.dedup();
        let suffix = result
            .player
            .actual_rating
            .map_or_else(String::new, |rating| format!("；来源原始 Rating {rating}"));
        lines.push(format!(
            "拟合B50: 使用拟合定数重算单曲ra并排序；新曲版本 {}{}",
            if versions.is_empty() {
                "未知".to_owned()
            } else {
                versions.join("、")
            },
            suffix
        ));
    }
    if options.include_chart_metadata {
        let matched = result
            .b35
            .iter()
            .chain(&result.b15)
            .filter(|chart| chart.fit_constant.is_some())
            .count();
        lines.push(format!(
            "谱面拟合: catalog 匹配 {matched}/{} 首",
            result.total_count()
        ));
        append_fit_index(&mut lines, result)?;
    }
    if let Some(summary) = options.summary() {
        lines.push(format!("筛选/排序: {summary}"));
    }
    match options.section {
        Section::Split => {
            append_section(&mut lines, "旧曲 B35", options.visible_b35(result), options);
            append_section(&mut lines, "新曲 B15", options.visible_b15(result), options);
        }
        Section::B35 => {
            append_section(&mut lines, "旧曲 B35", options.visible_b35(result), options)
        }
        Section::B15 => {
            append_section(&mut lines, "新曲 B15", options.visible_b15(result), options)
        }
        Section::B50 => append_section(&mut lines, "B50", options.visible(result), options),
    }
    Ok(lines.join("\n"))
}

pub fn pretty_json(value: &Value) -> Result<String, ScoreQueryToolError> {
    serde_json::to_string_pretty(value).map_err(|_| ScoreQueryToolError::internal())
}

pub fn batch(value: &Value) -> String {
    let counts = &value["counts"];
    let mut lines = vec![
        format!(
            "批量 B50 查询完成：请求 {} 个，成功 {} 个，失败 {} 个。",
            counts["requested"].as_u64().unwrap_or(0),
            counts["success"].as_u64().unwrap_or(0),
            counts["failure"].as_u64().unwrap_or(0),
        ),
        String::new(),
        "| 序号 | QQ | 水鱼昵称 | Rating | B50 ra | 状态 |".to_owned(),
        "| --- | --- | --- | ---: | ---: | --- |".to_owned(),
    ];
    for (index, item) in value["results"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let status = if item["ok"].as_bool() == Some(true) {
            "OK".to_owned()
        } else {
            format!(
                "{}: {}",
                item["error"]["code"].as_str().unwrap_or("ERROR"),
                item["error"]["message"].as_str().unwrap_or("查询失败")
            )
        };
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} |",
            index + 1,
            cell(item["qq"].as_str().unwrap_or("-")),
            cell(item["player"]["nickname"].as_str().unwrap_or("-")),
            display(&item["rating"]),
            display(&item["b50Rating"]),
            cell(&status),
        ));
    }
    lines.join("\n")
}

fn append_section(
    lines: &mut Vec<String>,
    title: &str,
    charts: Vec<&B50Chart>,
    options: &DisplayOptions,
) {
    lines.push(String::new());
    lines.push(format!("{title}："));
    if charts.is_empty() {
        lines.push(if options.summary().is_some() {
            "筛选后没有匹配歌曲。".to_owned()
        } else {
            "（没有符合条件的成绩）".to_owned()
        });
        return;
    }
    lines.extend(
        charts
            .into_iter()
            .enumerate()
            .map(|(index, chart)| song_line(chart, index + 1, options.include_chart_metadata)),
    );
}

fn song_line(chart: &B50Chart, rank: usize, include_metadata: bool) -> String {
    let mut details = vec![
        Some(difficulty(chart.key.difficulty()).to_owned()),
        Some(chart.level.clone()),
        chart
            .constant
            .map(|value| format!("定数 {}", constant(value))),
        chart
            .achievements
            .map(|value| format!("{}%", achievement(value))),
    ];
    if let Some(original) = chart.original_rating {
        details.push(chart.rating.map(|value| format!("拟合ra {value}")));
        details.push(Some(format!("原ra {original}")));
    } else {
        details.push(chart.rating.map(|value| format!("ra {value}")));
    }
    details.extend([
        chart.grade.as_ref().map(|value| value.to_ascii_uppercase()),
        chart
            .full_combo
            .as_ref()
            .map(|value| value.as_str().to_ascii_uppercase()),
        chart
            .full_sync
            .as_ref()
            .map(|value| value.as_str().to_ascii_uppercase()),
    ]);
    if include_metadata && let Some(fit) = chart.fit_constant {
        details.insert(3, Some(format!("拟合 {}", constant(fit))));
        details.insert(
            4,
            chart.fit_label.map(|label| match label {
                FitLabel::Inflated => "虚高".to_owned(),
                FitLabel::Deflated => "虚低".to_owned(),
                FitLabel::Equal => "持平".to_owned(),
            }),
        );
    }
    let details = details
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" / ");
    format!(
        "{rank}. [{}] {} - {details}",
        generation(chart.key.generation()),
        chart.title
    )
}

fn append_fit_index(
    lines: &mut Vec<String>,
    result: &B50Result,
) -> Result<(), ScoreQueryToolError> {
    let index = result.fit_index;
    if !index.available() {
        lines.push(format!(
            "虚高指数: 数据不足（匹配 0/{}）",
            index.b50.counted + index.b50.missing
        ));
        return Ok(());
    }
    let label = index.label.map(fit_index_label).unwrap_or("—");
    let suffix = if index.b50.missing > 0 {
        format!(
            " 匹配 {}/{}",
            index.b50.counted,
            index.b50.counted + index.b50.missing
        )
    } else {
        String::new()
    };
    lines.push(format!(
        "虚高指数: {} / {label}{suffix}",
        fit_metrics(index.b50)?
    ));
    let mut sections = Vec::new();
    if index.b35.counted > 0 {
        sections.push(format!("B35 {}", fit_metrics(index.b35)?));
    }
    if index.b15.counted > 0 {
        sections.push(format!("B15 {}", fit_metrics(index.b15)?));
    }
    if !sections.is_empty() {
        lines.push(format!("  └ {}", sections.join("；")));
    }
    Ok(())
}

fn fit_metrics(value: FitIndexSection) -> Result<String, ScoreQueryToolError> {
    let rating = value
        .virtual_rating
        .map(|value| format!("{value:+}.0 ra"))
        .unwrap_or_default();
    let ratio = value
        .virtual_ratio_percent
        .map(|value| signed_ratio(value, 2).map(|value| format!("{value}%")))
        .transpose()?
        .unwrap_or_default();
    Ok([rating, ratio]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" / "))
}

fn signed_ratio(value: ExactRatio, digits: u32) -> Result<String, ScoreQueryToolError> {
    let numerator = Decimal::try_from_i128_with_scale(value.numerator(), 0)
        .map_err(|_| ScoreQueryToolError::internal())?;
    let denominator =
        i128::try_from(value.denominator()).map_err(|_| ScoreQueryToolError::internal())?;
    let denominator = Decimal::try_from_i128_with_scale(denominator, 0)
        .map_err(|_| ScoreQueryToolError::internal())?;
    let ratio = numerator
        .checked_div(denominator)
        .ok_or_else(ScoreQueryToolError::internal)?
        .round_dp_with_strategy(digits, RoundingStrategy::MidpointNearestEven);
    let width = usize::try_from(digits).map_err(|_| ScoreQueryToolError::internal())?;
    let rendered = format!("{ratio:.width$}");
    Ok(if ratio.is_sign_negative() {
        rendered
    } else {
        format!("+{rendered}")
    })
}

const fn fit_index_label(value: FitIndexLabel) -> &'static str {
    match value {
        FitIndexLabel::ClearlyInflated => "明显虚高（水）",
        FitIndexLabel::SlightlyInflated => "略微虚高",
        FitIndexLabel::Balanced => "基本持平",
        FitIndexLabel::SlightlyDeflated => "略微虚低",
        FitIndexLabel::ClearlyDeflated => "明显虚低（硬实力）",
    }
}

fn append_identity(lines: &mut Vec<String>, lookup: &Lookup, identity: Option<&IdentityRecord>) {
    if let Lookup::Qq(qq) = lookup {
        lines.push(format!("QQ: {qq}"));
    }
    let Some(identity) = identity else { return };
    if let Some(name) = identity
        .qq_nickname
        .as_deref()
        .or(identity.friend_nickname.as_deref())
    {
        lines.push(format!("QQ昵称: {name}"));
    }
    if let Some(group) = &identity.preferred_group {
        let name = group
            .card
            .as_deref()
            .or(group.nickname.as_deref())
            .unwrap_or(&group.group_nickname);
        let suffix = group
            .group_name
            .as_deref()
            .map_or_else(String::new, |value| format!("（{value}）"));
        lines.push(format!("QQ群昵称: {name}{suffix}"));
    }
}

fn generation(value: maimai_core::ChartGeneration) -> &'static str {
    match value {
        maimai_core::ChartGeneration::Standard => "SD",
        maimai_core::ChartGeneration::Deluxe => "DX",
        maimai_core::ChartGeneration::UtageOnePlayer => "宴1P",
        maimai_core::ChartGeneration::UtageTwoPlayer => "宴2P",
    }
}

fn difficulty(value: maimai_core::Difficulty) -> &'static str {
    match value {
        maimai_core::Difficulty::Basic => "Basic",
        maimai_core::Difficulty::Advanced => "Advanced",
        maimai_core::Difficulty::Expert => "Expert",
        maimai_core::Difficulty::Master => "Master",
        maimai_core::Difficulty::ReMaster => "Re:MASTER",
        maimai_core::Difficulty::Utage => "Utage",
    }
}

fn constant(value: maimai_core::ChartConstant) -> String {
    let mut value = value.value().normalize().to_string();
    if !value.contains('.') {
        value.push_str(".0");
    }
    value
}

fn achievement(value: maimai_core::PlayAchievement) -> String {
    value
        .decimal_string()
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn display(value: &Value) -> String {
    if value.is_null() {
        "-".to_owned()
    } else {
        value.to_string()
    }
}
