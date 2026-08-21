use std::path::PathBuf;

use maimai_app::{
    score_list::{ScoreListImage, ScoreListTarget},
    scores::Lookup,
};
use maimai_core::{PlayerUsername, QqId, ScoreSource};
use serde_json::json;
use time::OffsetDateTime;

use super::{ScoreListSurface, convert, dto::ScoreListArgs, format};

fn args(value: serde_json::Value) -> Result<ScoreListArgs, serde_json::Error> {
    serde_json::from_value(value)
}

#[test]
fn frozen_surfaces_retain_exact_score_list_property_order() -> Result<(), Box<dyn std::error::Error>>
{
    for (source, expected) in [
        (
            crate::render_tools::MAIN_CONTRACT_JSON,
            vec!["qq", "username", "rating", "level", "ds", "page", "source"],
        ),
        (
            crate::render_tools::PUBLIC_CONTRACT_JSON,
            vec!["qq", "username", "rating", "level", "ds", "page"],
        ),
    ] {
        let contract =
            crate::contract::SurfaceContract::parse(source)?.retain_tools(&[super::TOOL_NAME]);
        assert_eq!(contract.tools().len(), 1);
        let tool = &contract.tools()[0];
        assert_eq!(tool.name(), super::TOOL_NAME);
        let properties = tool.input_schema()["properties"]
            .as_object()
            .ok_or("properties missing")?;
        assert_eq!(
            properties.keys().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(tool.input_schema()["required"], json!([]));
    }
    Ok(())
}

#[test]
fn identity_is_strictly_exactly_one() -> Result<(), Box<dyn std::error::Error>> {
    let now = OffsetDateTime::UNIX_EPOCH;
    for value in [
        json!({"rating":"14"}),
        json!({"qq":"1","username":"u","rating":"14"}),
    ] {
        assert!(convert::request(args(value)?, ScoreListSurface::Main, now).is_err());
    }
    let qq = convert::request(
        args(json!({"qq":"00123","rating":"14"}))?,
        ScoreListSurface::Main,
        now,
    )?;
    assert_eq!(qq.lookup, Lookup::Qq(QqId::new("00123")?));
    let user = convert::request(
        args(json!({"username":" alice ","rating":"14"}))?,
        ScoreListSurface::Main,
        now,
    )?;
    assert_eq!(user.lookup, Lookup::Username(PlayerUsername::new("alice")?));
    Ok(())
}

#[test]
fn target_precedence_and_numeric_type_matrix_are_exact() -> Result<(), Box<dyn std::error::Error>> {
    let now = OffsetDateTime::UNIX_EPOCH;
    let cases = [
        (
            json!({"qq":"1","ds":14,"rating":"13+","level":"12"}),
            ScoreListTarget::constant(maimai_core::ChartConstant::from_decimal_str("14.0")?),
        ),
        (json!({"qq":"1","rating":14}), ScoreListTarget::level("14")?),
        (
            json!({"qq":"1","rating":14.0}),
            ScoreListTarget::level("14")?,
        ),
        (
            json!({"qq":"1","rating":"14.0"}),
            ScoreListTarget::constant(maimai_core::ChartConstant::from_decimal_str("14.0")?),
        ),
        (
            json!({"qq":"1","rating":14.6}),
            ScoreListTarget::constant(maimai_core::ChartConstant::from_decimal_str("14.6")?),
        ),
        (
            json!({"qq":"1","level":"14+"}),
            ScoreListTarget::level("14+")?,
        ),
    ];
    for (value, expected) in cases {
        let request = convert::request(args(value)?, ScoreListSurface::Main, now)?;
        assert_eq!(request.target, expected);
    }
    assert!(
        convert::request(
            args(json!({"qq":"1","ds":"14.0"}))?,
            ScoreListSurface::Main,
            now,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn page_and_source_surface_rules_are_explicit() -> Result<(), Box<dyn std::error::Error>> {
    let now = OffsetDateTime::UNIX_EPOCH;
    for page in [json!(0), json!(-1), json!("1.5")] {
        assert!(
            convert::request(
                args(json!({"qq":"1","level":"14","page":page}))?,
                ScoreListSurface::Main,
                now,
            )
            .is_err()
        );
    }
    let main = convert::request(
        args(json!({
            "qq":"1","level":"14","source":"缓存",
            "data_source":"lxns","dataSource":"local",
            "score_source":"sy","scoreSource":"落雪"
        }))?,
        ScoreListSurface::Main,
        now,
    )?;
    assert_eq!(main.source, Some(ScoreSource::Lxns));
    for key in [
        "source",
        "scoreSource",
        "score_source",
        "dataSource",
        "data_source",
    ] {
        let mut value = json!({"qq":"1","level":"14"});
        value[key] = json!("sy");
        let error = convert::request(args(value)?, ScoreListSurface::Public, now)
            .err()
            .ok_or("public source should fail")?;
        assert!(error.to_string().starts_with("INVALID_INPUT:"));
    }
    let public = convert::request(
        args(json!({"qq":"1","level":"14"}))?,
        ScoreListSurface::Public,
        now,
    )?;
    assert_eq!(public.source, Some(ScoreSource::DivingFish));
    let fallback = convert::request(
        args(json!({"qq":"1","rating":"  ","level":"13+"}))?,
        ScoreListSurface::Main,
        now,
    )?;
    assert_eq!(fallback.target, ScoreListTarget::level("13+")?);
    let first_unknown = convert::request(
        args(json!({
            "qq":"1","level":"14","scoreSource":"unknown","source":"sy"
        }))?,
        ScoreListSurface::Main,
        now,
    );
    assert!(first_unknown.is_err());
    Ok(())
}

#[test]
fn main_has_source_metadata_and_public_is_path_only() -> Result<(), Box<dyn std::error::Error>> {
    let image = ScoreListImage {
        image_path: PathBuf::from("/tmp/list.png"),
        width: 1_400,
        height: 2_454,
        source: ScoreSource::Lxns,
        placeholder_covers: 0,
    };
    let main: serde_json::Value =
        serde_json::from_str(&format::success(&image, ScoreListSurface::Main)?)?;
    assert_eq!(main["scoreSource"], "lxns");
    assert_eq!(main["scoreSourceLabel"], "落雪");
    assert!(
        main["caption"]
            .as_str()
            .is_some_and(|text| text.contains("当前数据源：落雪"))
    );
    let public: serde_json::Value =
        serde_json::from_str(&format::success(&image, ScoreListSurface::Public)?)?;
    assert_eq!(public.as_object().map(serde_json::Map::len), Some(4));
    assert!(public.get("caption").is_none());
    Ok(())
}

#[test]
fn missing_assets_have_one_stable_error_code_without_configured_paths() {
    let error =
        super::error::ScoreListToolError::from(maimai_app::score_list::ScoreListError::Render(
            maimai_render::ScoreListRenderError::AssetsRequired {
                missing: vec!["mai/pic/design.png".to_owned()],
            },
        ));
    assert_eq!(
        error.to_string(),
        "SCORE_LIST_ASSETS_REQUIRED: missing score-list assets: [\"mai/pic/design.png\"]"
    );
}
