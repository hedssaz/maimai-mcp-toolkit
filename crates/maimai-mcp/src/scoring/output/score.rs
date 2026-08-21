use maimai_core::scoring::{PerNoteScores, ScoreCountResult};
use serde_json::{Map, Value, json};

use super::{judgment_groups, note_totals, totals};

pub(crate) fn score_count_value(result: &ScoreCountResult) -> Value {
    let mut output = Map::new();
    output.insert("display_digits".to_owned(), json!(result.display_digits));
    output.insert("note_totals".to_owned(), note_totals(&result.note_totals));
    output.insert(
        "rows".to_owned(),
        Value::Array(
            result
                .rows
                .iter()
                .map(|row| {
                    let mut value = Map::new();
                    value.insert("note_type".to_owned(), json!(row.note_type.as_str()));
                    value.insert("judgment".to_owned(), json!(row.judgment));
                    value.insert("count".to_owned(), json!(row.count));
                    value.insert("per_note".to_owned(), per_note(&row.per_note));
                    let mut contribution = score_fields(&row.contribution);
                    if let Some(selected) = row.selected_contribution {
                        contribution.insert("selected_mode".to_owned(), json!(selected));
                    }
                    value.insert("contribution".to_owned(), Value::Object(contribution));
                    Value::Object(value)
                })
                .collect(),
        ),
    );
    output.insert("totals".to_owned(), totals(&result.totals));
    output.insert("judgment_groups".to_owned(), judgment_groups());
    if let Some(mode) = result.score_mode {
        output.insert("score_mode".to_owned(), json!(mode.as_str()));
    }
    Value::Object(output)
}

fn per_note(value: &PerNoteScores) -> Value {
    Value::Object(score_fields(value))
}

fn score_fields(value: &PerNoteScores) -> Map<String, Value> {
    let mut output = Map::new();
    output.insert("base".to_owned(), json!(value.base));
    output.insert("break_bonus".to_owned(), json!(value.break_bonus));
    output.insert("oldscore".to_owned(), json!(value.old_score));
    output.insert("dxscore".to_owned(), json!(value.dx_score));
    output
}
