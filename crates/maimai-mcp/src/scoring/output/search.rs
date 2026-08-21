use maimai_core::scoring::{
    DisplayMode, Judgment, JudgmentSet, MetricDetails, NoteType, PerTypeSummary, SearchRequest,
    SearchResult, SearchSolution, TargetRange,
};
use serde_json::{Map, Value, json};

use super::{
    counts, displayed_allowed, exact_number, judgment_groups, note_totals, percentage, totals,
};
use crate::scoring::error::AdapterError;

pub(crate) fn search_value(
    request: &SearchRequest,
    result: &SearchResult,
) -> Result<Value, AdapterError> {
    if request.score_mode.is_achievement() {
        achievement_value(request, result)
    } else {
        raw_value(request, result)
    }
}

fn raw_value(request: &SearchRequest, result: &SearchResult) -> Result<Value, AdapterError> {
    let mut output = Map::new();
    output.insert("found".to_owned(), json!(result.found));
    output.insert("score_mode".to_owned(), json!(result.score_mode.as_str()));
    output.insert(
        "target_range".to_owned(),
        target_range(&result.target_range),
    );
    output.insert(
        "matching_score_count".to_owned(),
        json!(result.matching_metric_count),
    );
    output.insert(
        "matching_combination_count".to_owned(),
        exact_number(&result.matching_combination_count)?,
    );
    output.insert(
        "matching_combination_count_is_exact".to_owned(),
        json!(true),
    );
    output.insert(
        "returned_solution_count".to_owned(),
        json!(result.returned_solution_count),
    );
    output.insert("truncated".to_owned(), json!(result.truncated));
    output.insert(
        "solutions".to_owned(),
        Value::Array(result.solutions.iter().map(raw_solution).collect()),
    );
    output.insert(
        "per_type_summary".to_owned(),
        Value::Array(raw_summaries(request, result)?),
    );
    output.insert("judgment_groups".to_owned(), judgment_groups());
    insert_shortcuts(&mut output, result);
    Ok(Value::Object(output))
}

fn achievement_value(
    request: &SearchRequest,
    result: &SearchResult,
) -> Result<Value, AdapterError> {
    let mode = request.score_mode.as_str();
    let range = target_range(&result.target_range);
    let metric = result.metric_details.as_ref();
    let early_empty = metric.is_some_and(|metric| metric.max_loss < metric.min_loss);
    let mut output = Map::new();
    output.insert("found".to_owned(), json!(result.found));
    output.insert("score_mode".to_owned(), json!(mode));
    output.insert("target_type".to_owned(), json!(mode));
    output.insert(
        "display_mode".to_owned(),
        json!(display_mode(request.display_mode)),
    );
    output.insert("display_digits".to_owned(), json!(request.display_digits));
    output.insert("note_totals".to_owned(), note_totals(&request.note_totals));
    output.insert("target_range".to_owned(), range.clone());
    output.insert(format!("{mode}_range"), range);
    if let Some(details) = metric {
        output.insert(format!("{mode}_metric"), metric_value(details));
    }
    if early_empty {
        output.insert("solutions".to_owned(), Value::Array(Vec::new()));
        output.insert("judgment_groups".to_owned(), judgment_groups());
        return Ok(Value::Object(output));
    }
    output.insert(
        format!("matching_{mode}_count"),
        json!(result.matching_metric_count),
    );
    output.insert(
        "matching_combination_count".to_owned(),
        exact_number(&result.matching_combination_count)?,
    );
    output.insert(
        "matching_combination_count_is_exact".to_owned(),
        json!(true),
    );
    output.insert(
        "returned_solution_count".to_owned(),
        json!(result.returned_solution_count),
    );
    output.insert("truncated".to_owned(), json!(result.truncated));
    output.insert(
        "solutions".to_owned(),
        Value::Array(
            result
                .solutions
                .iter()
                .map(|solution| achievement_solution(mode, solution))
                .collect(),
        ),
    );
    output.insert(
        "per_type_summary".to_owned(),
        Value::Array(
            result
                .per_type_summary
                .iter()
                .map(|summary| achievement_summary(summary, mode))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    output.insert("judgment_groups".to_owned(), judgment_groups());
    insert_shortcuts(&mut output, result);
    Ok(Value::Object(output))
}

fn raw_solution(solution: &SearchSolution) -> Value {
    let mut output = Map::new();
    output.insert("score".to_owned(), json!(solution.metric));
    output.insert("counts".to_owned(), counts(&solution.counts));
    output.insert("totals".to_owned(), totals(&solution.totals));
    Value::Object(output)
}

fn achievement_solution(mode: &str, solution: &SearchSolution) -> Value {
    let mut output = Map::new();
    output.insert(
        mode.to_owned(),
        solution.percentage.as_ref().map_or(Value::Null, percentage),
    );
    output.insert(format!("{mode}_metric"), json!(solution.metric));
    output.insert("loss_metric".to_owned(), json!(solution.loss_metric));
    output.insert("counts".to_owned(), counts(&solution.counts));
    output.insert("totals".to_owned(), totals(&solution.totals));
    Value::Object(output)
}

fn raw_summaries(
    request: &SearchRequest,
    result: &SearchResult,
) -> Result<Vec<Value>, AdapterError> {
    NoteType::ALL
        .into_iter()
        .map(|note_type| {
            if let Some(summary) = result
                .per_type_summary
                .iter()
                .find(|summary| summary.note_type == note_type)
            {
                return raw_summary(summary);
            }
            let mut output = Map::new();
            output.insert("note_type".to_owned(), json!(note_type.as_str()));
            output.insert("possible_score_count".to_owned(), json!(1));
            output.insert("combination_count".to_owned(), json!(1));
            output.insert(
                "sample_combination_count".to_owned(),
                json!(usize::from(request.max_solutions > 0)),
            );
            output.insert("min_possible_score".to_owned(), json!(0));
            output.insert("max_possible_score".to_owned(), json!(0));
            output.insert(
                "allowed_judgments".to_owned(),
                json!(displayed_allowed(
                    note_type,
                    effective_allowed(request, note_type)
                )),
            );
            Ok(Value::Object(output))
        })
        .collect()
}

fn raw_summary(summary: &PerTypeSummary) -> Result<Value, AdapterError> {
    let mut output = Map::new();
    output.insert("note_type".to_owned(), json!(summary.note_type.as_str()));
    output.insert(
        "possible_score_count".to_owned(),
        json!(summary.possible_metric_count),
    );
    output.insert(
        "combination_count".to_owned(),
        exact_number(&summary.combination_count)?,
    );
    output.insert(
        "sample_combination_count".to_owned(),
        json!(summary.sample_combination_count),
    );
    output.insert(
        "min_possible_score".to_owned(),
        json!(summary.min_possible_metric),
    );
    output.insert(
        "max_possible_score".to_owned(),
        json!(summary.max_possible_metric),
    );
    output.insert(
        "allowed_judgments".to_owned(),
        json!(summary.allowed_judgments),
    );
    Ok(Value::Object(output))
}

fn achievement_summary(summary: &PerTypeSummary, _mode: &str) -> Result<Value, AdapterError> {
    let mut output = Map::new();
    output.insert("note_type".to_owned(), json!(summary.note_type.as_str()));
    output.insert(
        "possible_loss_count".to_owned(),
        json!(summary.possible_metric_count),
    );
    output.insert(
        "combination_count".to_owned(),
        exact_number(&summary.combination_count)?,
    );
    output.insert(
        "sample_combination_count".to_owned(),
        json!(summary.sample_combination_count),
    );
    output.insert(
        "min_possible_loss".to_owned(),
        json!(summary.min_possible_metric),
    );
    output.insert(
        "max_possible_loss".to_owned(),
        json!(summary.max_possible_metric),
    );
    output.insert(
        "allowed_judgments".to_owned(),
        json!(summary.allowed_judgments),
    );
    Ok(Value::Object(output))
}

fn metric_value(details: &MetricDetails) -> Value {
    let mut output = Map::new();
    output.insert("max_base".to_owned(), json!(details.max_base));
    if let Some(value) = details.max_break_bonus {
        output.insert("max_break_bonus".to_owned(), json!(value));
    }
    if let Some(value) = details.max_old {
        output.insert("max_old".to_owned(), json!(value));
    }
    output.insert("denominator".to_owned(), json!(details.denominator));
    output.insert("max_metric".to_owned(), json!(details.max_metric));
    if let Some(value) = details.base_weight {
        output.insert("base_weight".to_owned(), json!(value));
    }
    if let Some(value) = details.break_bonus_weight {
        output.insert("break_bonus_weight".to_owned(), json!(value));
    }
    output.insert(
        "loss_range".to_owned(),
        json!({"min": details.min_loss, "max": details.max_loss}),
    );
    Value::Object(output)
}

fn target_range(range: &TargetRange) -> Value {
    match range {
        TargetRange::Raw {
            min_score,
            max_score,
        } => json!({"min_score": min_score, "max_score": max_score}),
        TargetRange::Percentage {
            min,
            max,
            max_exclusive,
            mode_detail,
        } => {
            let mut output = Map::new();
            output.insert("min".to_owned(), json!(min));
            output.insert("max_exclusive".to_owned(), json!(max_exclusive));
            output.insert("max".to_owned(), json!(max));
            output.insert("mode_detail".to_owned(), json!(mode_detail));
            Value::Object(output)
        }
    }
}

fn effective_allowed(request: &SearchRequest, note_type: NoteType) -> JudgmentSet {
    let mut allowed = request.constraints.allowed[note_type.index()]
        .difference(request.constraints.disallowed[note_type.index()]);
    if request.constraints.no_miss_good {
        allowed.remove(Judgment::Miss);
        allowed.remove(Judgment::Good);
    }
    allowed
}

fn insert_shortcuts(output: &mut Map<String, Value>, result: &SearchResult) {
    let shortcuts = &result.shortcut_constraints;
    if !shortcuts.no_miss_good && shortcuts.break_max_perfect_or_below.is_none() {
        return;
    }
    let mut value = Map::new();
    if shortcuts.no_miss_good {
        value.insert("no_miss_good".to_owned(), json!(true));
    }
    if let Some(cap) = shortcuts.break_max_perfect_or_below {
        value.insert("break_max_perfect_or_below".to_owned(), json!(cap));
        value.insert(
            "break_min_critical".to_owned(),
            json!(shortcuts.break_min_critical),
        );
    }
    output.insert("shortcut_constraints".to_owned(), Value::Object(value));
}

const fn display_mode(mode: DisplayMode) -> &'static str {
    match mode {
        DisplayMode::Floor => "floor",
        DisplayMode::HalfUp => "half_up",
        DisplayMode::Exact => "exact",
    }
}
