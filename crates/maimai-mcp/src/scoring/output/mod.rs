mod score;
mod search;

pub(crate) use score::score_count_value;
pub(crate) use search::search_value;

use maimai_core::scoring::{
    ExactCount, Judgment, JudgmentCounts, JudgmentSet, NoteTotals, NoteType, PercentageDetails,
    ScoreTotals, SelectedTotal,
};
use serde_json::{Map, Value, json};

use super::error::AdapterError;

fn exact_number(value: &ExactCount) -> Result<Value, AdapterError> {
    serde_json::from_str(&value.to_string()).map_err(AdapterError::from)
}

fn note_totals(value: &NoteTotals) -> Value {
    let mut output = Map::new();
    for note_type in NoteType::ALL {
        output.insert(note_type.as_str().to_owned(), json!(value.get(note_type)));
    }
    Value::Object(output)
}

fn percentage(value: &PercentageDetails) -> Value {
    let mut output = Map::new();
    output.insert("raw".to_owned(), json!(value.raw));
    output.insert("display_floor".to_owned(), json!(value.display_floor));
    output.insert("display_half_up".to_owned(), json!(value.display_half_up));
    output.insert("scaled_floor".to_owned(), json!(value.scaled_floor));
    output.insert("scaled_half_up".to_owned(), json!(value.scaled_half_up));
    Value::Object(output)
}

fn totals(value: &ScoreTotals) -> Value {
    let mut output = Map::new();
    output.insert("base".to_owned(), json!(value.base));
    output.insert("break_bonus".to_owned(), json!(value.break_bonus));
    output.insert("oldscore".to_owned(), json!(value.old_score));
    output.insert("dxscore".to_owned(), json!(value.dx_score));
    output.insert(
        "oldacc".to_owned(),
        value
            .old_achievement
            .as_ref()
            .map_or(Value::Null, percentage),
    );
    output.insert(
        "dxacc".to_owned(),
        value
            .dx_achievement
            .as_ref()
            .map_or(Value::Null, percentage),
    );
    if let Some(selected) = &value.selected_mode {
        output.insert(
            "selected_mode".to_owned(),
            match selected {
                SelectedTotal::Raw(value) => json!(value),
                SelectedTotal::Percentage(value) => percentage(value),
            },
        );
    }
    Value::Object(output)
}

fn counts(value: &JudgmentCounts) -> Value {
    let mut output = Map::new();
    for note_type in NoteType::ALL {
        let mut row = Map::new();
        for judgment in Judgment::ALL {
            let count = value.get(note_type, judgment);
            if count == 0 {
                continue;
            }
            let name = judgment.display_name(note_type).to_owned();
            let current = row
                .get(&name)
                .and_then(Value::as_u64)
                .map_or(0, |value| value);
            row.insert(name, json!(current + u64::from(count)));
        }
        output.insert(note_type.as_str().to_owned(), Value::Object(row));
    }
    Value::Object(output)
}

fn judgment_groups() -> Value {
    let groups: [(&str, &[&str]); 10] = [
        ("all", &ALL),
        ("any", &ALL),
        ("ap", &PERFECT_OR_CRITICAL),
        ("fc", &NOT_MISS),
        ("fc_plus", &FC_PLUS),
        ("great", &GREAT),
        ("no_miss", &NOT_MISS),
        ("not_miss", &NOT_MISS),
        ("perfect", &PERFECT),
        ("perfect_or_critical", &PERFECT_OR_CRITICAL),
    ];
    let mut output = Map::new();
    for (name, judgments) in groups {
        output.insert(name.to_owned(), json!(judgments));
    }
    Value::Object(output)
}

fn displayed_allowed(note_type: NoteType, judgments: JudgmentSet) -> Vec<String> {
    let mut values = judgments
        .iter()
        .map(|judgment| judgment.display_name(note_type).to_owned())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

const ALL: [&str; 8] = [
    "critical",
    "good",
    "great_high",
    "great_low",
    "great_mid",
    "miss",
    "perfect_high",
    "perfect_low",
];
const PERFECT_OR_CRITICAL: [&str; 3] = ["critical", "perfect_high", "perfect_low"];
const NOT_MISS: [&str; 7] = [
    "critical",
    "good",
    "great_high",
    "great_low",
    "great_mid",
    "perfect_high",
    "perfect_low",
];
const FC_PLUS: [&str; 6] = [
    "critical",
    "great_high",
    "great_low",
    "great_mid",
    "perfect_high",
    "perfect_low",
];
const GREAT: [&str; 3] = ["great_high", "great_low", "great_mid"];
const PERFECT: [&str; 2] = ["perfect_high", "perfect_low"];

#[cfg(test)]
mod tests {
    use maimai_core::scoring::ExactCount;

    use super::exact_number;

    #[test]
    fn exact_count_stays_a_json_number_beyond_u128() -> Result<(), Box<dyn std::error::Error>> {
        let factor = ExactCount::from(10_000_000_000_000_000_000_u64);
        let mut value = factor.clone();
        for _ in 0..5 {
            value = value.multiplied(&factor);
        }
        let json = exact_number(&value)?;
        assert!(json.is_number());
        assert_eq!(json.to_string(), value.to_string());
        Ok(())
    }
}
