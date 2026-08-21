use std::{error::Error, path::Path};

use maimai_catalog::CatalogFiles;
use serde_json::{Map, Value, json};

use super::execute;
use crate::catalog_tools::format;

fn object(value: Value) -> Result<Map<String, Value>, Box<dyn Error>> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| std::io::Error::other("fixture must be an object").into())
}

#[test]
fn catalog_scoring_resolves_one_real_chart_and_keeps_direct_mode() -> Result<(), Box<dyn Error>> {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let snapshot = CatalogFiles::main_from_data_dir(data).load()?;
    let (resolved, options) = execute(
        &snapshot,
        "find_score_combinations",
        object(json!({
            "query":"id10574",
            "song_type":"dx",
            "difficulty":"Basic",
            "score_mode":"dxscore",
            "target_score":0,
            "allowed_judgments":{
                "tap":"miss",
                "touch":"miss",
                "hold":"miss",
                "slide":"miss",
                "break":"miss"
            },
            "max_solutions":0
        }))?,
    )?;
    assert_eq!(resolved["calculated"], true);
    assert_eq!(resolved["lookup"]["resolved"], true);
    assert_eq!(resolved["lookup"]["song"]["title"], "Selector");
    assert_eq!(resolved["lookup"]["chart"]["chart_type"], "dx");
    assert_eq!(resolved["lookup"]["chart"]["difficulty"], "Basic");
    assert!(
        resolved["lookup"]["note_totals"]["tap"]
            .as_u64()
            .is_some_and(|value| value > 0)
    );
    assert_eq!(resolved["matching_combination_count"], 1);
    let text = format::render("find_score_combinations", &resolved, options)?;
    assert!(
        text.contains("谱面: Selector ID 574 | DX Basic"),
        "unexpected text: {text}"
    );

    let (direct, options) = execute(
        &snapshot,
        "find_score_combinations",
        object(json!({
            "note_totals":{"tap":1},
            "score_mode":"dxscore",
            "target_score":0,
            "allowed_judgments":{"tap":"miss"},
            "max_solutions":0,
            "query":"this lookup must be ignored",
            "format":"json"
        }))?,
    )?;
    assert!(direct.get("lookup").is_none());
    assert_eq!(direct["matching_combination_count"], 1);
    assert_eq!(
        serde_json::from_str::<Value>(&format::render(
            "find_score_combinations",
            &direct,
            options
        )?)?,
        direct
    );
    Ok(())
}

#[test]
fn catalog_scoring_returns_candidates_instead_of_guessing() -> Result<(), Box<dyn Error>> {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let snapshot = CatalogFiles::main_from_data_dir(data).load()?;
    let (multiple, options) = execute(
        &snapshot,
        "find_score_combinations",
        object(json!({"query":"Link"}))?,
    )?;
    assert_eq!(multiple["calculated"], false);
    assert!(matches!(
        multiple["reason"].as_str(),
        Some("multiple_song_matches" | "multiple_chart_matches")
    ));
    assert!(format::render("find_score_combinations", &multiple, options)?.starts_with("未计算:"));

    let error = execute(
        &snapshot,
        "find_score_combinations",
        object(json!({"score_mode":"dxscore"}))?,
    )
    .err()
    .ok_or("missing lookup input was accepted")?;
    assert_eq!(
        error.to_string(),
        "note_totals or query/song_id/title is required"
    );
    Ok(())
}

#[test]
fn score_counts_uses_catalog_text_and_raw_formats() -> Result<(), Box<dyn Error>> {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let snapshot = CatalogFiles::main_from_data_dir(data).load()?;
    let arguments = json!({
        "counts":{"tap":{"great":2}},
        "display_digits":4
    });
    let (value, options) = execute(&snapshot, "score_counts", object(arguments.clone())?)?;
    let text = format::render("score_counts", &value, options)?;
    assert!(text.starts_with("计分结果:\n- oldscore:"));
    assert!(text.contains("- tap.great x2:"));

    let mut raw = object(arguments)?;
    raw.insert("include_raw".to_owned(), Value::Bool(true));
    let (value, options) = execute(&snapshot, "score_counts", raw)?;
    assert_eq!(
        serde_json::from_str::<Value>(&format::render("score_counts", &value, options)?)?,
        value
    );
    Ok(())
}
