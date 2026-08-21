use std::{error::Error, fs, io, path::PathBuf, sync::Arc, time::Duration};

use maimai_app::{
    completion::{
        CompletionCapabilities, CompletionImage, CompletionResponse, CompletionService,
        PlateBatchResult, RatingTableImage,
    },
    image_output::{ImageOutputPolicy, ImageOutputStore},
    oauth::OAuthService,
    score_service::PlayerScoreService,
};
use maimai_catalog::{CatalogFiles, CatalogStore, PlateName, PlateQuery, PlateServer};
use maimai_core::{AchievementRate, QqId, ScoreSource};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_render::CompletionRenderer;
use maimai_storage::{PlayerRecord, StateStore};
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use url::Url;

use super::{
    CompletionDispatcher, CompletionSurface, TOOL_NAMES, convert,
    dto::{IdentityDto, PlateBatchArgs, PlateItemDto, ProgressArgs, RatingArgs, Scalar},
    format,
};
use maimai_app::completion::CompletionTarget;
use maimai_render::FullSyncStatus;
use serde_json::Number;

fn identity() -> IdentityDto {
    IdentityDto {
        qq: Some(Scalar::Text("123456".to_owned())),
        ..IdentityDto::default()
    }
}

#[test]
fn invalid_batch_item_keeps_its_original_index() {
    let args = PlateBatchArgs {
        identity: identity(),
        items: vec![
            PlateItemDto::Version(Scalar::Text("真".to_owned())),
            PlateItemDto::Invalid(serde::de::IgnoredAny),
        ],
        ..PlateBatchArgs::default()
    };
    let error = convert::plate_batch(args, CompletionSurface::Main)
        .err()
        .map(|error| error.to_string());
    assert_eq!(
        error.as_deref(),
        Some("items[2] 必须是版本字符串、数字或牌子对象")
    );
}

#[test]
fn progress_normalizes_numeric_level_and_fsd() -> Result<(), Box<dyn std::error::Error>> {
    let request = convert::progress(
        ProgressArgs {
            identity: identity(),
            level: Some(Scalar::Number(Number::from(14))),
            plan: Some("fsd".to_owned()),
            ..ProgressArgs::default()
        },
        CompletionSurface::Main,
    )?;
    assert_eq!(request.level, "14");
    assert_eq!(
        request.target,
        CompletionTarget::FullSync(FullSyncStatus::FullSyncDeluxe)
    );
    Ok(())
}

#[test]
fn rating_validates_identity_level_mode_and_main_source_priority()
-> Result<(), Box<dyn std::error::Error>> {
    let request = convert::rating(
        serde_json::from_value::<RatingArgs>(json!({
            "qq":"123456","username":"alice","rating":"14"
        }))?,
        CompletionSurface::Main,
    );
    assert!(request.is_err());

    let request = convert::rating(
        serde_json::from_value::<RatingArgs>(json!({
            "qq":"123456","rating":15,"isfc":true,
            "scoreSource":"local","score_source":"lxns","source":"sy"
        }))?,
        CompletionSurface::Main,
    )?;
    assert_eq!(request.level, "15");
    assert_eq!(request.mode, maimai_render::RatingTableMode::FullCombo);
    assert_eq!(request.identity.source, Some(ScoreSource::Local));

    for alias in [
        "source",
        "scoreSource",
        "score_source",
        "dataSource",
        "data_source",
    ] {
        let mut value = json!({"qq":"123456","rating":"14"});
        value[alias] = json!("lxns");
        let aliased = convert::rating(
            serde_json::from_value::<RatingArgs>(value)?,
            CompletionSurface::Main,
        )?;
        assert_eq!(aliased.identity.source, Some(ScoreSource::Lxns));
    }

    let username = convert::rating(
        serde_json::from_value::<RatingArgs>(json!({
            "username":"alice","rating":"14+","source":"not-a-source"
        }))?,
        CompletionSurface::Main,
    )?;
    assert_eq!(username.identity.source, None);
    assert!(matches!(
        username.identity.lookup,
        maimai_app::scores::Lookup::Username(_)
    ));

    let default_level = convert::rating(
        serde_json::from_value::<RatingArgs>(json!({"qq":"123456"}))?,
        CompletionSurface::Main,
    )?;
    assert_eq!(default_level.level, "14");
    for valid in ["7", "7+", "8+", "14+", "15"] {
        let value = convert::rating(
            serde_json::from_value::<RatingArgs>(json!({"qq":"123456","rating":valid}))?,
            CompletionSurface::Main,
        )?;
        assert_eq!(value.level, valid);
    }
    for invalid in ["6+", "15+", "14.0", "  "] {
        let value = convert::rating(
            serde_json::from_value::<RatingArgs>(json!({"qq":"123456","rating":invalid}))?,
            CompletionSurface::Main,
        );
        assert!(value.is_err(), "{invalid} must be rejected");
    }
    Ok(())
}

#[test]
fn public_rating_rejects_every_private_source_alias() -> Result<(), Box<dyn std::error::Error>> {
    for alias in [
        "source",
        "scoreSource",
        "score_source",
        "dataSource",
        "data_source",
    ] {
        let mut value = json!({"qq":"123456","rating":"14"});
        value[alias] = json!("sy");
        let result = convert::rating(
            serde_json::from_value::<RatingArgs>(value)?,
            CompletionSurface::Public,
        );
        assert!(result.is_err(), "{alias} must be rejected on public");
    }
    Ok(())
}

#[test]
fn public_rejects_source_and_jp_capabilities() {
    let source = convert::progress(
        ProgressArgs {
            identity: IdentityDto {
                qq: Some(Scalar::Text("123456".to_owned())),
                source: Some("sy".to_owned()),
                ..IdentityDto::default()
            },
            ..ProgressArgs::default()
        },
        CompletionSurface::Public,
    );
    assert!(source.is_err());

    let jp = convert::progress(
        ProgressArgs {
            identity: identity(),
            server: Some("jp".to_owned()),
            ..ProgressArgs::default()
        },
        CompletionSurface::Public,
    );
    assert!(jp.is_err());
}

#[test]
fn frozen_surfaces_keep_six_completion_tools_and_exact_rating_schema() -> Result<(), Box<dyn Error>>
{
    let common = json!({
        "qq":{"type":"string","description":"玩家 QQ 号。和 username 二选一。"},
        "username":{"type":"string","description":"查分器用户名。和 qq 二选一。"},
        "rating":{"type":"string","description":"等级，如 14+、15"},
        "isfc":{"type":"boolean","description":"是否按FC筛选"},
    });
    let mut main_properties = common.clone();
    main_properties["source"] = json!({
        "type":"string","enum":["local","sy","lxns"],
        "description":"可选，临时指定成绩数据源。local=只查本地缓存，sy=只查水鱼，lxns=只查落雪 OAuth 缓存；不传时使用该 QQ 的默认偏好。"
    });
    for (contract, expected_properties) in [
        (crate::render_tools::MAIN_CONTRACT_JSON, main_properties),
        (crate::render_tools::PUBLIC_CONTRACT_JSON, common),
    ] {
        let contract = crate::contract::SurfaceContract::parse(contract)?.retain_tools(&TOOL_NAMES);
        assert_eq!(
            contract
                .tools()
                .iter()
                .map(|tool| tool.name())
                .collect::<Vec<_>>(),
            TOOL_NAMES
        );
        let rating = contract
            .tools()
            .iter()
            .find(|tool| tool.name() == "render_maimai_rating")
            .ok_or("rating tool missing")?;
        assert_eq!(
            Value::Object(rating.input_schema().clone()),
            json!({"type":"object","properties":expected_properties,"required":["rating"]})
        );
        for name in [
            "render_maimai_plate_batch",
            "render_maimai_plate_progress_batch",
        ] {
            let tool = contract
                .tools()
                .iter()
                .find(|tool| tool.name() == name)
                .ok_or("batch tool missing")?;
            assert_eq!(tool.input_schema()["properties"]["items"]["maxItems"], 50);
            assert_eq!(
                tool.input_schema()["properties"]["versions"]["maxItems"],
                50
            );
        }
    }
    Ok(())
}

#[test]
fn image_and_partial_batch_json_keep_legacy_fields() -> Result<(), Box<dyn Error>> {
    let response = CompletionResponse {
        value: CompletionImage {
            index: 1,
            version: "真".to_owned(),
            target: "极".to_owned(),
            server: PlateServer::Cn,
            path: PathBuf::from("/tmp/plate.png"),
            width: 1_400,
            height: 2_110,
        },
        source: ScoreSource::Local,
    };
    let main: Value = serde_json::from_str(&format::image(&response, CompletionSurface::Main)?)?;
    assert_eq!(main["imagePath"], "/tmp/plate.png");
    assert_eq!(main["scoreSource"], "local");
    assert!(
        main["caption"]
            .as_str()
            .is_some_and(|value| value.starts_with("当前数据源：本地缓存"))
    );
    let public: Value =
        serde_json::from_str(&format::image(&response, CompletionSurface::Public)?)?;
    assert!(public.get("scoreSource").is_none());

    let rating = CompletionResponse {
        value: RatingTableImage {
            path: PathBuf::from("/tmp/rating.png"),
            width: 1_400,
            height: 940,
        },
        source: ScoreSource::DivingFish,
    };
    let main_rating: Value =
        serde_json::from_str(&format::rating_image(&rating, CompletionSurface::Main)?)?;
    assert_eq!(main_rating["scoreSource"], "sy");
    assert!(main_rating["caption"].is_string());
    let public_rating: Value =
        serde_json::from_str(&format::rating_image(&rating, CompletionSurface::Public)?)?;
    assert_eq!(
        public_rating,
        json!({"imagePath":"/tmp/rating.png","mimeType":"image/png","width":1400,"height":940})
    );

    let batch = PlateBatchResult {
        results: vec![response.value],
        errors: vec![maimai_app::completion::CompletionItemError {
            index: 2,
            label: "不存在将".to_owned(),
            message: "未找到牌子".to_owned(),
        }],
        source: Some(ScoreSource::Local),
    };
    let value: Value =
        serde_json::from_str(&format::plate_batch(&batch, CompletionSurface::Main)?)?;
    assert_eq!(value["results"], value["images"]);
    assert_eq!(value["results"][0]["label"], "真极");
    assert_eq!(value["errors"][0]["index"], "2");
    Ok(())
}

#[tokio::test]
async fn real_duplex_renders_plate_and_partial_batch_without_structured_content()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    let catalog = Arc::new(
        CatalogStore::load(CatalogFiles::from_data_dir(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        ))
        .await?,
    );
    let state = StateStore::open(temp.path().join("state.db")).await?;
    let members = catalog.snapshot().plate_members(&PlateQuery::new(
        PlateName::new("雪峰")?,
        PlateServer::Custom,
    ));
    let member = members.members().first().ok_or("custom member missing")?;
    let chart = member.charts().first().ok_or("custom chart missing")?;
    let key = chart.key().cloned().ok_or("custom chart key missing")?;
    let title = member.title().to_owned();
    let level = chart.level().to_owned();
    let progress_level = level.clone();
    let constant = chart.constant();
    state
        .upsert_record(&PlayerRecord {
            qq: QqId::new("123456")?,
            chart: key,
            title,
            level: Some(level),
            level_label: Some("Basic".to_owned()),
            ds: constant,
            achievements: Some(AchievementRate::from_decimal_str("100")?.into()),
            dx_score: Some(1_000),
            fc: Some(maimai_core::FullComboStatus::FullCombo),
            fs: Some(maimai_core::FullSyncStatus::FullSync),
            rate: Some("sss".to_owned()),
            ra: Some(280),
            version: Some("PRiSM".to_owned()),
            is_new: false,
            score_source: ScoreSource::Local,
            source_detail: None,
            raw: None,
            payload: json!({}),
            updated_at: "2026-08-18T00:00:00Z".to_owned(),
        })
        .await?;
    let base = Url::parse("http://127.0.0.1:9/")?;
    let scores = Arc::new(PlayerScoreService::with_lxns(
        state.clone(),
        Arc::clone(&catalog),
        DivingFishScoreClient::new(DivingFishClient::with_base_urls(
            base.join("api/")?.as_str(),
            base.join("covers/")?.as_str(),
        )?),
        OAuthService::new(
            state,
            LxnsOAuthClient::new(OAuthConfig::new(
                "client-id",
                None,
                None,
                base.join("authorize")?,
                base.join("token")?,
                vec!["read_player".to_owned()],
            )?)?,
        ),
        LxnsScoreEndpoint::new(base.join("lxns/")?, Duration::from_millis(100))?,
    ));
    let cache = temp.path().join("covers");
    fs::create_dir(&cache)?;
    let static_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../maimaidx_render_mcp/static");
    let service = Arc::new(CompletionService::new(
        Arc::clone(&catalog),
        Arc::clone(&scores),
        CompletionRenderer::new(&static_root, &cache)?,
        ImageOutputStore::new(temp.path().join("images"), ImageOutputPolicy::standard())?,
        CompletionCapabilities::main(),
    ));
    let completion = CompletionDispatcher::new(Arc::clone(&service), CompletionSurface::Main)?;
    let dispatcher = crate::PairDispatcher::new(&TOOL_NAMES, completion.clone(), completion);
    let contract =
        crate::contract::SurfaceContract::parse(crate::render_tools::MAIN_CONTRACT_JSON)?
            .retain_tools(&TOOL_NAMES);
    let server = crate::ContractServer::new(contract, dispatcher);
    let responses = duplex_calls(
        server,
        vec![
            (
                "render_maimai_plate",
                json!({
                    "qq":"123456","version":"雪峰","plan":"将","server":"custom","source":"local"
                }),
            ),
            (
                "render_maimai_plate_batch",
                json!({
                    "qq":"123456","source":"local","items":[
                        {"version":"雪峰","plan":"极","server":"custom"},
                        {"version":"不存在","plan":"将","server":"custom"}
                    ]
                }),
            ),
            (
                "render_maimai_rating",
                json!({"qq":"123456","source":"local","rating":"14","isfc":false}),
            ),
            (
                "render_maimai_progress",
                json!({
                    "qq":"123456","source":"local","level":progress_level,"plan":"sss"
                }),
            ),
            (
                "render_maimai_plate_progress",
                json!({
                    "qq":"123456","source":"local","version":"雪峰","plan":"将","server":"custom"
                }),
            ),
            (
                "render_maimai_plate_progress_batch",
                json!({
                    "qq":"123456","source":"local","items":[
                        {"version":"雪峰","plan":"将","server":"custom"},
                        {"version":"真","plan":"将","server":"cn"}
                    ]
                }),
            ),
        ],
    )
    .await?;
    let single = &responses[0];
    assert_eq!(single["result"]["isError"], false, "{single:#}");
    assert!(single["result"].get("structuredContent").is_none());
    let single_payload = content_json(single)?;
    assert_eq!(single_payload["width"], 1_400);
    assert!(PathBuf::from(single_payload["imagePath"].as_str().ok_or("path missing")?).is_file());

    let batch = &responses[1];
    assert_eq!(batch["result"]["isError"], false);
    let batch_payload = content_json(batch)?;
    assert_eq!(batch_payload["results"].as_array().map(Vec::len), Some(1));
    assert_eq!(batch_payload["errors"][0]["index"], "2");

    let rating = &responses[2];
    assert_eq!(rating["result"]["isError"], false, "{rating:#}");
    assert!(rating["result"].get("structuredContent").is_none());
    let rating_payload = content_json(rating)?;
    assert_eq!(rating_payload["width"], 1_400);
    assert_eq!(rating_payload["scoreSource"], "local");
    assert!(PathBuf::from(rating_payload["imagePath"].as_str().ok_or("path missing")?).is_file());

    let progress = &responses[3];
    assert_eq!(progress["result"]["isError"], false, "{progress:#}");
    assert!(progress["result"].get("structuredContent").is_none());
    let progress_payload = content_json(progress)?;
    assert_eq!(progress_payload["mimeType"], "image/png");

    let progress_text = &responses[4];
    assert_eq!(
        progress_text["result"]["isError"], false,
        "{progress_text:#}"
    );
    assert!(progress_text["result"].get("structuredContent").is_none());
    let text = content_text(progress_text)?;
    assert!(text.starts_with("您的「雪峰将」剩余进度如下："));
    assert!(text.contains("当前数据源：本地缓存"));

    let progress_batch = &responses[5];
    assert_eq!(
        progress_batch["result"]["isError"], false,
        "{progress_batch:#}"
    );
    assert!(progress_batch["result"].get("structuredContent").is_none());
    let progress_batch_payload = content_json(progress_batch)?;
    let progress_results = progress_batch_payload["results"]
        .as_array()
        .ok_or("progress batch results missing")?;
    assert_eq!(progress_results.len(), 2);
    assert!(progress_results[0]["text"].is_string());
    assert_eq!(progress_results[1]["mimeType"], "image/png");

    let public_service = Arc::new(CompletionService::new(
        catalog,
        scores,
        CompletionRenderer::new(&static_root, &cache)?,
        ImageOutputStore::new(
            temp.path().join("public-images"),
            ImageOutputPolicy::standard(),
        )?,
        CompletionCapabilities::public(),
    ));
    let public = CompletionDispatcher::new(public_service, CompletionSurface::Public)?;
    let public_pair = crate::PairDispatcher::new(&TOOL_NAMES, public.clone(), public);
    let public_contract =
        crate::contract::SurfaceContract::parse(crate::render_tools::PUBLIC_CONTRACT_JSON)?
            .retain_tools(&TOOL_NAMES);
    let public_response = duplex_calls(
        crate::ContractServer::new(public_contract, public_pair),
        vec![
            (
                "render_maimai_plate",
                json!({"qq":"123456","version":"熊","plan":"将","server":"jp"}),
            ),
            (
                "render_maimai_plate",
                json!({"qq":"123456","version":"真","plan":"将","source":"local"}),
            ),
            (
                "render_maimai_rating",
                json!({"qq":"123456","rating":"14","source":"sy"}),
            ),
        ],
    )
    .await?;
    assert_eq!(public_response[0]["result"]["isError"], true);
    assert_eq!(public_response[1]["result"]["isError"], true);
    assert_eq!(public_response[2]["result"]["isError"], true);
    Ok(())
}

async fn duplex_calls(
    server: crate::ContractServer<
        crate::PairDispatcher<CompletionDispatcher, CompletionDispatcher>,
    >,
    calls: Vec<(&str, Value)>,
) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
    let (client, server_io) = tokio::io::duplex(256 * 1024);
    let server_task = tokio::spawn(async move {
        let running = rmcp::serve_server(server, server_io)
            .await
            .map_err(|error| io::Error::other(error.to_string()))?;
        match running
            .waiting()
            .await
            .map_err(|error| io::Error::other(error.to_string()))?
        {
            QuitReason::JoinError(error) => Err(io::Error::other(error.to_string())),
            _ => Ok(()),
        }
    });
    let (read, mut write) = tokio::io::split(client);
    let mut lines = BufReader::new(read).lines();
    rpc(
        &mut write,
        &mut lines,
        1,
        "initialize",
        json!({"protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"completion-test","version":"1"}}),
    )
    .await?;
    let mut results = Vec::new();
    for (offset, (name, arguments)) in calls.into_iter().enumerate() {
        results.push(
            rpc(
                &mut write,
                &mut lines,
                offset as u64 + 2,
                "tools/call",
                json!({"name":name,"arguments":arguments}),
            )
            .await?,
        );
    }
    write.shutdown().await?;
    server_task.await??;
    Ok(results)
}

async fn rpc(
    write: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    lines: &mut Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let message = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    write
        .write_all(serde_json::to_string(&message)?.as_bytes())
        .await?;
    write.write_all(b"\n").await?;
    write.flush().await?;
    loop {
        let line = lines.next_line().await?.ok_or("server closed")?;
        let value: Value = serde_json::from_str(&line)?;
        if value["id"] == id {
            return Ok(value);
        }
    }
}

fn content_json(response: &Value) -> Result<Value, Box<dyn Error + Send + Sync>> {
    Ok(serde_json::from_str(content_text(response)?)?)
}

fn content_text(response: &Value) -> Result<&str, Box<dyn Error + Send + Sync>> {
    response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| "content text missing".into())
}
