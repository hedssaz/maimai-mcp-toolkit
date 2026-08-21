use std::path::PathBuf;

use maimai_app::{
    rise_score::{RiseScoreAlgorithm, RiseScoreImage},
    scores::Lookup,
};
use maimai_core::{PlayerUsername, QqId, ScoreSource};
use serde_json::json;
use time::OffsetDateTime;

use super::{RiseScoreSurface, convert, dto::RiseScoreArgs, format};

fn args(value: serde_json::Value) -> Result<RiseScoreArgs, serde_json::Error> {
    serde_json::from_value(value)
}

#[test]
fn frozen_surfaces_keep_exact_rise_score_schema() -> Result<(), Box<dyn std::error::Error>> {
    for (source, expected) in [
        (
            crate::render_tools::MAIN_CONTRACT_JSON,
            vec!["qq", "username", "level", "score", "algorithm", "source"],
        ),
        (
            crate::render_tools::PUBLIC_CONTRACT_JSON,
            vec!["qq", "username", "level", "score", "algorithm"],
        ),
    ] {
        let contract =
            crate::contract::SurfaceContract::parse(source)?.retain_tools(&[super::TOOL_NAME]);
        assert_eq!(contract.tools().len(), 1);
        let properties = contract.tools()[0].input_schema()["properties"]
            .as_object()
            .ok_or("properties missing")?;
        assert_eq!(
            properties.keys().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(contract.tools()[0].input_schema()["required"], json!([]));
    }
    Ok(())
}

#[test]
fn identity_is_strictly_exactly_one() -> Result<(), Box<dyn std::error::Error>> {
    let now = OffsetDateTime::UNIX_EPOCH;
    for value in [json!({}), json!({"qq":"1","username":"u"})] {
        assert!(convert::request(args(value)?, RiseScoreSurface::Main, now).is_err());
    }
    let qq = convert::request(args(json!({"qq":"00123"}))?, RiseScoreSurface::Main, now)?;
    assert_eq!(qq.lookup, Lookup::Qq(QqId::new("00123")?));
    let username = convert::request(
        args(json!({"username":" alice "}))?,
        RiseScoreSurface::Main,
        now,
    )?;
    assert_eq!(
        username.lookup,
        Lookup::Username(PlayerUsername::new("alice")?)
    );
    Ok(())
}

#[test]
fn option_types_algorithms_and_source_precedence_are_exact()
-> Result<(), Box<dyn std::error::Error>> {
    let now = OffsetDateTime::UNIX_EPOCH;
    let request = convert::request(
        args(json!({
            "qq":"1", "level":"14+", "score":5, "algorithm":"EXPECTED",
            "source":"local", "data_source":"lxns", "dataSource":"local",
            "score_source":"sy", "scoreSource":"落雪"
        }))?,
        RiseScoreSurface::Main,
        now,
    )?;
    assert_eq!(request.level.as_deref(), Some("14+"));
    assert_eq!(request.score, Some(5));
    assert_eq!(request.algorithm, RiseScoreAlgorithm::Expected);
    assert_eq!(request.source, Some(ScoreSource::Lxns));
    for invalid in [
        json!({"qq":"1","score":"5"}),
        json!({"qq":"1","score":-1}),
        json!({"qq":"1","algorithm":"fixed"}),
        json!({"qq":"1","level":14}),
    ] {
        assert!(convert::request(args(invalid)?, RiseScoreSurface::Main, now).is_err());
    }
    let default = convert::request(args(json!({"qq":"1"}))?, RiseScoreSurface::Main, now)?;
    assert_eq!(default.algorithm, RiseScoreAlgorithm::Legacy);
    Ok(())
}

#[test]
fn public_rejects_every_source_alias_and_is_fixed_to_diving_fish()
-> Result<(), Box<dyn std::error::Error>> {
    let now = OffsetDateTime::UNIX_EPOCH;
    for key in [
        "source",
        "scoreSource",
        "score_source",
        "dataSource",
        "data_source",
    ] {
        let mut value = json!({"qq":"1"});
        value[key] = json!("sy");
        assert!(convert::request(args(value)?, RiseScoreSurface::Public, now).is_err());
    }
    let request = convert::request(
        args(json!({"username":"alice"}))?,
        RiseScoreSurface::Public,
        now,
    )?;
    assert_eq!(request.source, Some(ScoreSource::DivingFish));
    Ok(())
}

#[test]
fn main_output_has_caption_and_public_has_only_basic_image_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let image = RiseScoreImage {
        image_path: PathBuf::from("/tmp/rise.png"),
        width: 1_000,
        height: 960,
        source: ScoreSource::Lxns,
        placeholder_covers: 0,
    };
    let main: serde_json::Value =
        serde_json::from_str(&format::success(&image, RiseScoreSurface::Main)?)?;
    assert_eq!(main["scoreSource"], "lxns");
    assert_eq!(main["scoreSourceLabel"], "落雪");
    assert!(
        main["caption"]
            .as_str()
            .is_some_and(|value| value.contains("落雪"))
    );
    let public: serde_json::Value =
        serde_json::from_str(&format::success(&image, RiseScoreSurface::Public)?)?;
    assert_eq!(public.as_object().map(serde_json::Map::len), Some(4));
    assert!(public.get("caption").is_none());
    Ok(())
}

#[test]
fn application_errors_redact_provider_bodies_and_asset_roots() {
    let asset =
        super::error::RiseScoreToolError::from(maimai_app::rise_score::RiseScoreError::Render(
            maimai_render::RiseScoreRenderError::AssetsRequired {
                missing: vec!["mai/pic/title.png".to_owned()],
            },
        ));
    let text = asset.to_string();
    assert!(text.starts_with("RISE_SCORE_ASSETS_REQUIRED:"));
    assert!(!text.contains("secret"));
}
