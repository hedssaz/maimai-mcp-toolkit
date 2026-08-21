use serde_json::{Map, Value, json};

use super::{find_combinations_value, score_counts_value};

fn object(value: Value) -> Result<Map<String, Value>, Box<dyn std::error::Error>> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| std::io::Error::other("test fixture must be an object").into())
}

#[test]
fn score_counts_matches_standalone_shape() -> Result<(), Box<dyn std::error::Error>> {
    let value = score_counts_value(object(json!({
        "counts": {
            "tap": {"great": 2},
            "break": {"perfect_high": 1, "critical": 1}
        },
        "display_digits": 4
    }))?)?;
    let golden: Value = serde_json::from_str(include_str!("golden/score_counts.json"))?;
    assert_eq!(value, golden);

    Ok(())
}

#[test]
fn dxacc_count_only_matches_complete_python_golden() -> Result<(), Box<dyn std::error::Error>> {
    let value = find_combinations_value(object(json!({
        "note_totals": {"tap": 2, "break": 1},
        "score_mode": "dxacc",
        "target_acc": "100.5000%",
        "display_mode": "floor",
        "max_solutions": 0
    }))?)?;
    let golden: Value = serde_json::from_str(include_str!("golden/dxacc_count_only.json"))?;
    assert_eq!(value, golden);
    Ok(())
}

#[test]
fn aliases_convert_and_disagreements_are_tool_errors() -> Result<(), Box<dyn std::error::Error>> {
    let value = find_combinations_value(object(json!({
        "note_totals": {"tch": 1, "touch_hold": 1, "brk": 1},
        "score_mode": "dxscore",
        "target_score": 6,
        "allowed_judgments": {"tch": "ap", "touch_hold": ["perfect", "critical"]},
        "fixed_counts": {"brk": {"cp": 1}},
        "no_miss_good": true,
        "all_notes_no_miss_good": true,
        "break_max_perfect_or_below": 0,
        "max_break_non_critical": 0,
        "max_solutions": 0
    }))?)?;
    assert!(value["matching_combination_count"].is_number());

    let result = find_combinations_value(object(json!({
        "note_totals": {"break": 1},
        "target_score": 2600,
        "no_miss_good": true,
        "fc_plus_only": false
    }))?);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn exact_count_is_emitted_as_json_number() -> Result<(), Box<dyn std::error::Error>> {
    let value = find_combinations_value(object(json!({
        "note_totals": {"break": 2},
        "score_mode": "oldscore",
        "target_score": 5100,
        "max_solutions": 1
    }))?)?;
    assert!(value["matching_combination_count"].is_number());
    assert_eq!(value["matching_combination_count"], 2);
    assert_eq!(value["solutions"][0]["counts"]["break"]["perfect_high"], 2);
    assert_eq!(
        value["solutions"][0]["counts"]["break"].get("perfect_low"),
        None
    );
    assert_eq!(
        value["solutions"][0]["counts"]["break"].get("critical"),
        None
    );
    Ok(())
}
