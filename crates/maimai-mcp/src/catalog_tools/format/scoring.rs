use serde_json::Value;

use super::value::{array, criteria, fmt};

pub(super) fn score_counts(value: &Value) -> String {
    let totals = value.get("totals");
    let mut lines = vec![
        "计分结果:".to_owned(),
        format!(
            "- oldscore: {}",
            fmt(totals.and_then(|value| value.get("oldscore")), "-")
        ),
        format!(
            "- oldacc: {}",
            percent_details(totals.and_then(|value| value.get("oldacc")))
        ),
        format!(
            "- dxscore: {}",
            fmt(totals.and_then(|value| value.get("dxscore")), "-")
        ),
        format!(
            "- dxacc: {}",
            percent_details(totals.and_then(|value| value.get("dxacc")))
        ),
        format!(
            "- base: {}",
            fmt(totals.and_then(|value| value.get("base")), "-")
        ),
        format!(
            "- break_bonus: {}",
            fmt(totals.and_then(|value| value.get("break_bonus")), "-")
        ),
    ];
    if let Some(note_totals) = value
        .get("note_totals")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!(
            "物量: {}",
            note_totals
                .iter()
                .map(|(key, value)| format!("{key} {}", fmt(Some(value), "-")))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let rows = array(value.get("rows"));
    lines.extend(rows.iter().take(20).map(|row| {
        let contribution = row.get("contribution");
        format!(
            "- {}.{} x{}: oldscore {}, dxscore {}, base {}, bonus {}",
            fmt(row.get("note_type"), "-"),
            fmt(row.get("judgment"), "-"),
            fmt(row.get("count"), "-"),
            fmt(contribution.and_then(|value| value.get("oldscore")), "-"),
            fmt(contribution.and_then(|value| value.get("dxscore")), "-"),
            fmt(contribution.and_then(|value| value.get("base")), "-"),
            fmt(contribution.and_then(|value| value.get("break_bonus")), "-"),
        )
    }));
    if rows.len() > 20 {
        lines.push(format!("... 还有 {} 行明细未展开", rows.len() - 20));
    }
    lines.join("\n")
}

pub(super) fn find_combinations(value: &Value) -> String {
    let unresolved = value.get("resolved").and_then(Value::as_bool) == Some(false);
    let uncalculated_with_reason = value.get("calculated").and_then(Value::as_bool) == Some(false)
        && value.get("reason").is_some();
    if unresolved || uncalculated_with_reason {
        return lookup_failure(value);
    }
    let mut lines = vec![
        format!(
            "反查结果: found={} mode={} truncated={}",
            fmt(value.get("found"), "-"),
            fmt(value.get("score_mode"), "-"),
            fmt(value.get("truncated"), "False"),
        ),
        format!(
            "匹配分数数: {}; 返回组合: {}; 组合总数: {}",
            fmt(value.get("matching_score_count"), "-"),
            fmt(value.get("returned_solution_count"), "-"),
            fmt(value.get("matching_combination_count"), "-"),
        ),
    ];
    if let Some(lookup) = value.get("lookup") {
        let song = lookup.get("song");
        let chart = lookup.get("chart");
        lines.push(format!(
            "谱面: {} ID {} | {} {} Lv {} ds {}",
            fmt(song.and_then(|value| value.get("title")), "-"),
            fmt(song.and_then(|value| value.get("id")), "-"),
            fmt(chart.and_then(|value| value.get("chart_type")), "-").to_uppercase(),
            fmt(chart.and_then(|value| value.get("difficulty")), "-"),
            fmt(chart.and_then(|value| value.get("level")), "-"),
            fmt(chart.and_then(|value| value.get("ds")), "-"),
        ));
    }
    if let Some(target) = value.get("target_range") {
        lines.push(format!("目标: {}", criteria(Some(target))));
    }
    if let Some(target) = value.get("target_acc_range") {
        lines.push(format!("目标达成率: {}", criteria(Some(target))));
    }
    if let Some(shortcuts) = value.get("shortcut_constraints") {
        lines.push(format!("快捷约束: {}", criteria(Some(shortcuts))));
    }
    let solutions = array(value.get("solutions"));
    lines.extend(solutions.iter().enumerate().map(|(index, solution)| {
        let score = solution.get("score").or_else(|| {
            solution
                .get(
                    value
                        .get("score_mode")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                )
                .and_then(|mode| mode.get("display_floor").or_else(|| mode.get("raw")))
        });
        format!(
            "{}. score={} counts: {}",
            index + 1,
            fmt(score, "-"),
            counts(solution.get("counts")),
        )
    }));
    if solutions.is_empty() {
        lines.push("没有找到满足条件的判定组合。".to_owned());
    }
    lines.join("\n")
}

fn percent_details(value: Option<&Value>) -> String {
    let Some(value) = value.and_then(Value::as_object) else {
        return fmt(value, "-");
    };
    format!(
        "{} floor / {} half-up (raw {})",
        fmt(value.get("display_floor"), "-"),
        fmt(value.get("display_half_up"), "-"),
        fmt(value.get("raw"), "-"),
    )
}

fn lookup_failure(value: &Value) -> String {
    let mut lines = vec![
        format!("未计算: {}", fmt(value.get("reason"), "-")),
        format!("条件: {}", criteria(value.get("criteria"))),
    ];
    let songs = array(value.get("songs"));
    if !songs.is_empty() {
        lines.push("候选歌曲:".to_owned());
        lines.extend(songs.iter().take(20).map(|song| {
            format!(
                "- {} | ID {} | charts {}",
                fmt(song.get("title"), "-"),
                fmt(song.get("id"), "-"),
                fmt(song.get("matched_chart_count"), "-"),
            )
        }));
    }
    let charts = array(value.get("charts"));
    if !charts.is_empty() {
        lines.push("候选谱面:".to_owned());
        lines.extend(charts.iter().take(20).map(chart_line));
    }
    lines.join("\n")
}

fn chart_line(chart: &Value) -> String {
    format!(
        "- {} {} Lv {} ds {} | {}",
        fmt(chart.get("chart_type"), "-").to_uppercase(),
        fmt(chart.get("difficulty"), "-"),
        fmt(chart.get("level"), "-"),
        fmt(chart.get("ds"), "-"),
        fmt(chart.get("charter"), "-"),
    )
}

fn counts(value: Option<&Value>) -> String {
    let Some(value) = value.and_then(Value::as_object) else {
        return "-".to_owned();
    };
    let result = value
        .iter()
        .filter_map(|(note_type, judgments)| {
            let judgments = judgments.as_object()?;
            (!judgments.is_empty()).then(|| {
                format!(
                    "{note_type}({})",
                    judgments
                        .iter()
                        .map(|(judgment, count)| {
                            format!("{judgment}:{}", fmt(Some(count), "-"))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
        })
        .collect::<Vec<_>>()
        .join("; ");
    if result.is_empty() {
        "-".to_owned()
    } else {
        result
    }
}
