use std::{error::Error, fs, io, path::PathBuf, sync::Arc, time::Duration};

use maimai_app::{
    image_output::{ImageOutputPolicy, ImageOutputStore},
    music_score::{MusicScoreBatchResult, MusicScoreImage, MusicScoreItemError, MusicScoreService},
    oauth::OAuthService,
    score_service::PlayerScoreService,
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, QqId, ScoreSource,
    SongIdNamespace, SourceSongId,
};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_render::MusicScoreRenderer;
use maimai_storage::{PlayerProfile, PlayerRecord, StateStore};
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use url::Url;

use super::{
    MusicScoreDispatcher, MusicScoreSurface, TOOL_NAME, convert, dto::MusicScoreArgs, format,
    main_music_score_server,
};
use crate::render_tools::{MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON};

#[test]
fn frozen_surfaces_retain_their_exact_music_score_schema() -> Result<(), Box<dyn Error>> {
    let main: Value = serde_json::from_str(MAIN_CONTRACT_JSON)?;
    let public: Value = serde_json::from_str(PUBLIC_CONTRACT_JSON)?;
    let main_tool = tool(&main)?;
    let public_tool = tool(&public)?;
    assert!(
        main_tool["inputSchema"]["properties"]
            .get("source")
            .is_some()
    );
    assert!(
        public_tool["inputSchema"]["properties"]
            .get("source")
            .is_none()
    );
    for contract in [MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON] {
        let retained =
            crate::contract::SurfaceContract::parse(contract)?.retain_tools(&[TOOL_NAME]);
        assert_eq!(retained.tools().len(), 1);
        assert_eq!(retained.tools()[0].name(), TOOL_NAME);
    }
    Ok(())
}

#[test]
fn identity_is_strict_and_checked_before_music_resolution() -> Result<(), Box<dyn Error>> {
    for value in [json!({}), json!({"qq":"10001","username":"alice"})] {
        let args: MusicScoreArgs = serde_json::from_value(value)?;
        assert_eq!(
            convert::request(args, MusicScoreSurface::Main)
                .err()
                .map(|error| error.to_string()),
            Some("qq 和 username 必须且只能提供一个".to_owned())
        );
    }
    Ok(())
}

#[test]
fn aliases_follow_id_query_type_and_source_precedence() -> Result<(), Box<dyn Error>> {
    let args: MusicScoreArgs = serde_json::from_value(json!({
        "qq":"10001",
        "music_id":"10835",
        "musicId":"383",
        "id":"1",
        "query":"ignored",
        "songQuery":"also ignored",
        "songType":"dx",
        "scoreSource":"水鱼",
        "source":"local"
    }))?;
    let request = convert::request(args, MusicScoreSurface::Main)?;
    assert_eq!(request.music.query_label(), "10835");
    assert!(request.music.query.is_none());
    assert_eq!(
        request.music.chart_type,
        Some(maimai_app::music_info::MusicInfoChartType::Deluxe)
    );
    assert_eq!(request.source, Some(ScoreSource::DivingFish));

    let query_aliases: MusicScoreArgs = serde_json::from_value(json!({
        "qq":"10001","query":"first","songQuery":"second",
        "song_query":"third","title":"fourth"
    }))?;
    assert_eq!(
        convert::request(query_aliases, MusicScoreSurface::Main)?
            .music
            .query_label(),
        "first"
    );

    for alias in ["本地", "cache", "缓存", "local"] {
        let args: MusicScoreArgs = serde_json::from_value(json!({
            "qq":"10001","id":"383","data_source":alias
        }))?;
        assert_eq!(
            convert::request(args, MusicScoreSurface::Main)?.source,
            Some(ScoreSource::Local)
        );
    }
    let lxns: MusicScoreArgs = serde_json::from_value(json!({
        "qq":"10001","id":"383","source":"luoxue"
    }))?;
    assert_eq!(
        convert::request(lxns, MusicScoreSurface::Main)?.source,
        Some(ScoreSource::Lxns)
    );
    Ok(())
}

#[test]
fn public_rejects_source_aliases_and_defaults_to_diving_fish() -> Result<(), Box<dyn Error>> {
    let clean: MusicScoreArgs = serde_json::from_value(json!({"username":"alice","query":"Link"}))?;
    assert_eq!(
        convert::request(clean, MusicScoreSurface::Public)?.source,
        Some(ScoreSource::DivingFish)
    );
    for field in [
        "source",
        "scoreSource",
        "score_source",
        "dataSource",
        "data_source",
    ] {
        let mut value = json!({"qq":"10001","query":"Link"});
        value[field] = json!("sy");
        let args: MusicScoreArgs = serde_json::from_value(value)?;
        assert_eq!(
            convert::request(args, MusicScoreSurface::Public)
                .err()
                .map(|error| error.to_string()),
            Some("public 版本不接受 source 参数".to_owned())
        );
    }
    Ok(())
}

#[test]
fn main_and_public_single_outputs_are_exact() -> Result<(), Box<dyn Error>> {
    let result = MusicScoreBatchResult {
        images: vec![image(1, "ST")],
        errors: Vec::new(),
        source: ScoreSource::Local,
    };
    let main: Value = serde_json::from_str(&format::result(&result, MusicScoreSurface::Main)?)?;
    assert_eq!(main["imagePath"], "/tmp/music.png");
    assert_eq!(main["mimeType"], "image/png");
    assert_eq!(main["width"], 1_200);
    assert_eq!(main["height"], 900);
    assert_eq!(main["scoreSource"], "local");
    assert_eq!(main["scoreSourceLabel"], "本地缓存");
    assert!(
        main["caption"]
            .as_str()
            .is_some_and(|text| text.starts_with("当前数据源：本地缓存"))
    );

    let public: Value = serde_json::from_str(&format::result(&result, MusicScoreSurface::Public)?)?;
    assert_eq!(
        public,
        json!({"imagePath":"/tmp/music.png","mimeType":"image/png","width":1200,"height":900})
    );
    Ok(())
}

#[test]
fn partial_batch_keeps_results_images_and_errors() -> Result<(), Box<dyn Error>> {
    let result = MusicScoreBatchResult {
        images: vec![image(1, "ST")],
        errors: vec![MusicScoreItemError {
            index: 2,
            query: "Link".to_owned(),
            chart_type: Some("DX".to_owned()),
            message: "render failed".to_owned(),
        }],
        source: ScoreSource::DivingFish,
    };
    let value: Value = serde_json::from_str(&format::result(&result, MusicScoreSurface::Main)?)?;
    assert_eq!(value["results"], value["images"]);
    assert_eq!(value["images"][0]["chartType"], "ST");
    assert_eq!(value["errors"][0]["index"], "2");
    assert_eq!(value["errors"][0]["chartType"], "DX");
    assert_eq!(value["scoreSource"], "sy");
    Ok(())
}

#[tokio::test]
async fn duplex_renders_dual_and_dxdata_only_from_one_snapshot_per_call()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let service = service(&temp).await?;
    let server = main_music_score_server(
        MAIN_CONTRACT_JSON,
        MusicScoreDispatcher::main(Arc::new(service)),
    )?;
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
            "clientInfo":{"name":"music-score-test","version":"1"}}),
    )
    .await?;

    let dual = tool_call(
        &mut write,
        &mut lines,
        2,
        json!({"qq":"10001","query":"相信彩虹","source":"local"}),
    )
    .await?;
    assert_eq!(dual["result"]["isError"], false, "{dual}");
    assert!(dual["result"].get("structuredContent").is_none());
    let dual_value = content_json(&dual)?;
    let images = dual_value["images"].as_array().ok_or("images missing")?;
    assert_eq!(
        images
            .iter()
            .map(|image| image["chartType"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        ["ST", "DX"]
    );
    for image in images {
        assert_eq!(
            (image["width"].as_u64(), image["height"].as_u64()),
            (Some(1_200), Some(900))
        );
        assert!(PathBuf::from(image["imagePath"].as_str().ok_or("path missing")?).is_file());
    }

    let dxdata = tool_call(
        &mut write,
        &mut lines,
        3,
        json!({"qq":"10001","query":"Xaleid◆scopiX","source":"local"}),
    )
    .await?;
    assert_eq!(dxdata["result"]["isError"], false, "{dxdata}");
    assert!(!content_json(&dxdata)?["imagePath"].is_null());

    let identity = tool_call(
        &mut write,
        &mut lines,
        4,
        json!({"qq":"10001","username":"alice","query":"not-a-song"}),
    )
    .await?;
    assert_eq!(identity["result"]["isError"], true);
    assert_eq!(
        identity["result"]["content"][0]["text"],
        "qq 和 username 必须且只能提供一个"
    );

    write.shutdown().await?;
    drop(write);
    server_task.await??;
    Ok(())
}

async fn service(temp: &TempDir) -> Result<MusicScoreService, Box<dyn Error>> {
    let catalog = Arc::new(
        CatalogStore::load(CatalogFiles::from_data_dir(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        ))
        .await?,
    );
    let state = StateStore::open(temp.path().join("state.db")).await?;
    seed_local(&state).await?;
    let base = Url::parse("http://127.0.0.1:9/")?;
    let diving_fish = DivingFishClient::with_base_urls(
        base.join("api/")?.as_str(),
        base.join("covers/")?.as_str(),
    )?;
    let oauth = OAuthService::new(
        state.clone(),
        LxnsOAuthClient::new(OAuthConfig::new(
            "client-id",
            None,
            None,
            base.join("authorize")?,
            base.join("token")?,
            vec!["read_player".to_owned()],
        )?)?,
    );
    let scores = Arc::new(PlayerScoreService::with_lxns(
        state,
        Arc::clone(&catalog),
        DivingFishScoreClient::new(diving_fish),
        oauth,
        LxnsScoreEndpoint::new(base.join("lxns/")?, Duration::from_millis(100))?,
    ));
    let static_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../maimaidx_render_mcp/static");
    let cache = temp.path().join("cover-cache");
    fs::create_dir(&cache)?;
    Ok(MusicScoreService::new(
        catalog,
        scores,
        MusicScoreRenderer::new(static_root, cache)?,
        ImageOutputStore::new(temp.path().join("images"), ImageOutputPolicy::standard())?,
    ))
}

async fn seed_local(store: &StateStore) -> Result<(), Box<dyn Error>> {
    let qq = QqId::new("10001")?;
    store
        .upsert_profile(&PlayerProfile {
            qq: qq.clone(),
            nickname: Some("Local Tester".to_owned()),
            player_rating: Some(300),
            player_old_rating: None,
            player_new_rating: None,
            score_source: Some(ScoreSource::Local),
            source_detail: None,
            raw: None,
            updated_at: "2026-08-18T00:00:00Z".to_owned(),
        })
        .await?;
    store
        .upsert_record(&PlayerRecord {
            qq,
            chart: ChartKey::new(
                SourceSongId::numeric(SongIdNamespace::DivingFish, 8),
                ChartGeneration::Standard,
                Difficulty::Master,
            )?,
            title: "True Love Song".to_owned(),
            level: Some("12".to_owned()),
            level_label: Some("Master".to_owned()),
            ds: Some(ChartConstant::from_decimal_str("12.4")?),
            achievements: Some(AchievementRate::from_decimal_str("100.0")?.into()),
            dx_score: Some(1_000),
            fc: None,
            fs: None,
            rate: Some("sss".to_owned()),
            ra: Some(298),
            version: Some("PRiSM".to_owned()),
            is_new: false,
            score_source: ScoreSource::DivingFish,
            source_detail: None,
            raw: None,
            payload: json!({}),
            updated_at: "2026-08-18T00:00:00Z".to_owned(),
        })
        .await?;
    Ok(())
}

fn image(index: usize, chart_type: &str) -> MusicScoreImage {
    MusicScoreImage {
        index,
        query: "Link".to_owned(),
        music_id: "383".to_owned(),
        title: "Link".to_owned(),
        chart_type: chart_type.to_owned(),
        image_path: PathBuf::from("/tmp/music.png"),
        width: 1_200,
        height: 900,
    }
}

fn tool(contract: &Value) -> Result<&Value, Box<dyn Error>> {
    contract["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|tool| tool["name"] == TOOL_NAME))
        .ok_or_else(|| "music score tool missing".into())
}

fn content_json(message: &Value) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(
        message["result"]["content"][0]["text"]
            .as_str()
            .ok_or("tool text missing")?,
    )?)
}

async fn tool_call<W, R>(
    writer: &mut W,
    lines: &mut Lines<BufReader<R>>,
    id: u64,
    arguments: Value,
) -> Result<Value, io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: tokio::io::AsyncRead + Unpin,
{
    rpc(
        writer,
        lines,
        id,
        "tools/call",
        json!({"name":TOOL_NAME,"arguments":arguments}),
    )
    .await
}

async fn rpc<W, R>(
    writer: &mut W,
    lines: &mut Lines<BufReader<R>>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: tokio::io::AsyncRead + Unpin,
{
    let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    writer.write_all(request.to_string().as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    let line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "rpc response missing"))?;
    serde_json::from_str(&line).map_err(io::Error::other)
}
