use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use image::{ImageFormat, Rgba, RgbaImage};
use maimai_app::{
    b50_image::{
        B50ImageService, B50ImageStyle, OutputPolicy, OutputStore, ResourceDirectories, StyleStore,
    },
    b50_render::{B50RenderService, ResourceOverridePolicy},
    oauth::OAuthService,
    score_service::PlayerScoreService,
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_render::{LegacyAssets, LegacyRenderer};
use maimai_storage::StateStore;
use rmcp::service::QuitReason;
use secrecy::SecretString;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines},
    net::TcpListener,
};
use url::Url;

use crate::contract::SurfaceContract;

use super::{
    super::{MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON},
    B50RenderDispatcher, RenderDeployment, TOOL_NAME, b50_render_server, convert,
    dto::RenderB50Args,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn frozen_main_and_public_contracts_keep_exact_b50_schema() -> TestResult {
    let expected = [
        "qq",
        "username",
        "style",
        "computeFromRecords",
        "source",
        "staticDir",
        "coverCacheDir",
        "timeoutMs",
        "title",
    ];
    for source in [MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON] {
        let contract = SurfaceContract::parse(source)?.retain_tools(&[TOOL_NAME]);
        assert_eq!(contract.tools().len(), 1);
        let tool = &contract.tools()[0];
        assert_eq!(tool.name(), TOOL_NAME);
        let properties = tool.input_schema()["properties"]
            .as_object()
            .ok_or_else(|| io::Error::other("properties missing"))?;
        assert_eq!(
            properties.keys().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(tool.input_schema()["required"], json!([]));
    }
    let main = SurfaceContract::parse(MAIN_CONTRACT_JSON)?.retain_tools(&[TOOL_NAME]);
    let public = SurfaceContract::parse(PUBLIC_CONTRACT_JSON)?.retain_tools(&[TOOL_NAME]);
    assert!(
        main.tools()[0].input_schema()["properties"]["source"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("local/sy"))
    );
    assert!(
        !public.tools()[0].input_schema()["properties"]["source"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("local/sy"))
    );
    Ok(())
}

#[test]
fn aliases_default_style_and_public_source_policy_are_typed() -> TestResult {
    let now = time::OffsetDateTime::UNIX_EPOCH;
    let main: RenderB50Args = serde_json::from_value(json!({
        "qq":"10001","source":"fitted","style":"legacy"
    }))?;
    let prepared = convert::request(main, RenderDeployment::Main, now)?;
    assert!(matches!(
        prepared.request.mode,
        maimai_app::score_service::B50Mode::Computed(maimai_app::scores::RatingMode::Fit)
    ));
    assert_eq!(prepared.request.style, B50ImageStyle::Legacy);
    assert_eq!(prepared.request.source, None);

    let public: RenderB50Args = serde_json::from_value(json!({
        "username":"tester","source":"local"
    }))?;
    let prepared = convert::request(public, RenderDeployment::Public, now)?;
    assert_eq!(
        prepared.request.source,
        Some(maimai_core::ScoreSource::DivingFish)
    );
    assert_eq!(prepared.request.style, B50ImageStyle::Yuzu);

    let qq_wins: RenderB50Args = serde_json::from_value(json!({
        "qq":"10001","username":"ignored"
    }))?;
    let prepared = convert::request(qq_wins, RenderDeployment::Main, now)?;
    assert!(matches!(
        prepared.request.lookup,
        maimai_app::scores::Lookup::Qq(_)
    ));

    for invalid in [
        json!({"qq":"10001","source":"mystery"}),
        json!({"qq":"10001","style":" Yuzu "}),
    ] {
        let args: RenderB50Args = serde_json::from_value(invalid)?;
        assert!(convert::request(args, RenderDeployment::Main, now).is_err());
    }
    Ok(())
}

#[tokio::test]
async fn real_duplex_renders_three_styles_and_preserves_surface_output_differences() -> TestResult {
    let responses = vec![success(), records_success(), success(), success()];
    let (base, _) = mock_http(responses).await?;
    let fixture = fixture(base).await?;
    let main = B50RenderDispatcher::new(Arc::clone(&fixture.service), RenderDeployment::Main);
    let main_response = duplex_calls(
        b50_render_server(MAIN_CONTRACT_JSON, main)?,
        vec![
            json!({"qq":"10001","style":"legacy","source":"sy",
                "staticDir":fixture.static_root,"coverCacheDir":fixture.cover_root}),
            json!({"qq":"10001","style":"legacy","computeFromRecords":true}),
        ],
    )
    .await?;
    let main_call = &main_response[0];
    assert_eq!(main_call["result"]["isError"], false);
    assert!(main_call["result"].get("structuredContent").is_none());
    let main_payload = content_json(main_call)?;
    assert_eq!(
        (
            main_payload["width"].as_u64(),
            main_payload["height"].as_u64()
        ),
        (Some(1_920), Some(478))
    );
    assert!(
        main_payload["caption"]
            .as_str()
            .is_some_and(|value| value.starts_with("当前数据源：水鱼"))
    );
    assert!(main_payload["timings"]["query_ms"].is_number());
    assert_png(&main_payload)?;
    let computed_payload = content_json(&main_response[1])?;
    assert_eq!(computed_payload["width"], 1_920);
    assert!(
        computed_payload["caption"]
            .as_str()
            .is_some_and(|value| !value.starts_with("成绩更新时间："))
    );
    assert_png(&computed_payload)?;

    let public = B50RenderDispatcher::new(Arc::clone(&fixture.service), RenderDeployment::Public);
    let public_responses = duplex_calls(
        b50_render_server(PUBLIC_CONTRACT_JSON, public)?,
        vec![
            json!({"qq":"10001","style":"yuzu","source":"local"}),
            json!({"username":"tester","style":"maibot"}),
        ],
    )
    .await?;
    let yuzu = content_json(&public_responses[0])?;
    let maibot = content_json(&public_responses[1])?;
    assert_eq!(
        (yuzu["width"].as_u64(), yuzu["height"].as_u64()),
        (Some(1_400), Some(1_650))
    );
    assert_eq!(
        (maibot["width"].as_u64(), maibot["height"].as_u64()),
        (Some(1_400), Some(780))
    );
    for (response, payload) in public_responses.iter().zip([&yuzu, &maibot]) {
        assert_eq!(response["result"]["isError"], false);
        assert!(response["result"].get("structuredContent").is_none());
        assert!(payload.get("caption").is_none());
        assert!(payload.get("timings").is_none());
        assert_png(payload)?;
    }
    Ok(())
}

#[tokio::test]
async fn timeout_unsafe_path_and_provider_errors_are_structured_and_redacted() -> TestResult {
    let secret = "B50_SECRET_SENTINEL";
    let (base, _) = mock_http(vec![
        MockResponse {
            status: 200,
            delay: Duration::from_millis(1_200),
            body: success().body,
        },
        MockResponse {
            status: 500,
            delay: Duration::ZERO,
            body: json!({"token":secret,"qq":"10001"}).to_string(),
        },
    ])
    .await?;
    let fixture = fixture(base).await?;
    let dispatcher = B50RenderDispatcher::new(Arc::clone(&fixture.service), RenderDeployment::Main);
    let outside = fixture.temp.path().join("outside");
    fs::create_dir(&outside)?;
    let responses = duplex_calls(
        b50_render_server(MAIN_CONTRACT_JSON, dispatcher)?,
        vec![
            json!({"qq":"10001","staticDir":outside}),
            json!({"qq":"10001","timeoutMs":1000}),
            json!({"qq":"10001"}),
        ],
    )
    .await?;
    assert_error(&responses[0], "UNSAFE_PATH")?;
    assert_error(&responses[1], "TIMEOUT")?;
    assert_error(&responses[2], "PROVIDER_ERROR")?;
    assert!(!responses[2].to_string().contains(secret));
    Ok(())
}

struct Fixture {
    temp: TempDir,
    service: Arc<B50RenderService>,
    static_root: PathBuf,
    cover_root: PathBuf,
}

async fn fixture(base: Url) -> TestResult<Fixture> {
    let temp = TempDir::new()?;
    write_catalog(temp.path())?;
    let static_root = create_assets(temp.path())?;
    let cover_root = temp.path().join("cover-cache");
    fs::create_dir(&cover_root)?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    store
        .set_diving_fish_developer_token(
            &SecretString::from("fixture-developer-token"),
            time::OffsetDateTime::UNIX_EPOCH,
        )
        .await?;
    let catalog = Arc::new(CatalogStore::load(CatalogFiles::from_data_dir(temp.path())).await?);
    let diving_fish = DivingFishClient::with_base_urls(
        base.join("api/")?.as_str(),
        base.join("covers/")?.as_str(),
    )?;
    let oauth = OAuthService::new(
        store.clone(),
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
        store,
        catalog,
        DivingFishScoreClient::new(diving_fish),
        oauth,
        LxnsScoreEndpoint::new(base.join("lxns/")?, Duration::from_secs(1))?,
    ));
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf");
    let images = B50ImageService::new(
        StyleStore::new(temp.path().join("style.json"), B50ImageStyle::Yuzu)?,
        OutputStore::new(temp.path().join("images"), OutputPolicy::standard())?,
        LegacyRenderer::new(LegacyAssets::new(&font, &font))?,
        ResourceDirectories::new(&static_root, &cover_root)?,
    );
    let paths = ResourceOverridePolicy::new(
        static_root.clone(),
        vec![static_root.clone()],
        vec![cover_root.clone()],
    )?;
    Ok(Fixture {
        temp,
        service: Arc::new(B50RenderService::new(scores, images, paths)),
        static_root,
        cover_root,
    })
}

#[derive(Clone)]
struct MockResponse {
    status: u16,
    delay: Duration,
    body: String,
}

fn success() -> MockResponse {
    MockResponse {
        status: 200,
        delay: Duration::ZERO,
        body: json!({
            "nickname":"DF Tester","rating":600,"charts":{
                "sd":[{"song_id":383,"title":"Link(CoF)","type":"SD","level":"13",
                    "level_index":3,"ds":"13.0","achievements":"100.0","ra":280}],
                "dx":[{"song_id":10383,"title":"Link(CoF)","type":"DX","level":"13+",
                    "level_index":3,"ds":"13.8","achievements":"100.0","ra":298}]
            }
        })
        .to_string(),
    }
}

fn records_success() -> MockResponse {
    MockResponse {
        status: 200,
        delay: Duration::ZERO,
        body: json!({
            "nickname":"DF Tester","rating":600,"records":[
                {"song_id":383,"title":"Link(CoF)","type":"SD","level":"13",
                    "level_index":3,"ds":"13.0","achievements":"100.0","ra":280},
                {"song_id":10383,"title":"Link(CoF)","type":"DX","level":"13+",
                    "level_index":3,"ds":"13.8","achievements":"100.0","ra":298}
            ]
        })
        .to_string(),
    }
}

async fn mock_http(responses: Vec<MockResponse>) -> TestResult<(Url, ())> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let _ = read_request(&mut stream).await;
                tokio::time::sleep(response.delay).await;
                let reason = if response.status < 400 { "OK" } else { "Error" };
                let encoded = format!(
                    "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    response.body.len(),
                    response.body
                );
                let _ = stream.write_all(encoded.as_bytes()).await;
            });
        }
    });
    Ok((Url::parse(&format!("http://{address}/"))?, ()))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> io::Result<()> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
    }
}

fn write_catalog(root: &Path) -> io::Result<()> {
    let files = [
        (
            "lxns_song_list.json",
            json!({"songs":[{"id":383,"title":"Link","artist":"A",
            "genre":"maimai","bpm":150,"version":25000,"difficulties":{
                "standard":[{"difficulty":3,"level":"13","level_value":13.0,"notes":{}}],
                "dx":[{"difficulty":3,"level":"13+","level_value":13.8,"notes":{}}]}}],
            "genres":[],"versions":[{"title":"PRiSM","version":25000}]})
            .to_string(),
        ),
        (
            "divingfish_song_list.json",
            json!([
                df_song(383, "SD", 13.0, "13"),
                df_song(10383, "DX", 13.8, "13+")
            ])
            .to_string(),
        ),
        ("lxns_alias_list.json", r#"{"aliases":[]}"#.to_owned()),
        ("music_alias.json", r#"{"content":[]}"#.to_owned()),
        ("custom_aliases.json", "{}".to_owned()),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#.to_owned()),
        ("zh_s2t.json", "{}".to_owned()),
        (
            "divingfish_chart_stats.json",
            json!({"charts":{
            "383":[{},{},{},{"fit_diff":13.1}],"10383":[{},{},{},{"fit_diff":13.9}]}})
            .to_string(),
        ),
    ];
    for (name, content) in files {
        fs::write(root.join(name), content)?;
    }
    Ok(())
}

fn df_song(id: u32, kind: &str, constant: f64, level: &str) -> Value {
    json!({"id":id.to_string(),"title":"Link(CoF)","type":kind,"ds":[1,2,3,constant],
        "level":["1","2","3",level],"charts":[{},{},{},{}],
        "basic_info":{"bpm":150,"from":"PRiSM","is_new":true}})
}

fn create_assets(root: &Path) -> TestResult<PathBuf> {
    let static_root = root.join("static");
    let pic = static_root.join("mai/pic");
    let cover = static_root.join("mai/cover");
    fs::create_dir_all(&pic)?;
    fs::create_dir_all(&cover)?;
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf");
    fs::copy(&font, static_root.join("ResourceHanRoundedCN-Bold.ttf"))?;
    fs::copy(font, static_root.join("Torus SemiBold.otf"))?;
    for (name, width, height) in [
        ("b50_bg.png", 1400, 1650),
        ("b50_score_basic.png", 265, 105),
        ("b50_score_advanced.png", 265, 105),
        ("b50_score_expert.png", 265, 105),
        ("b50_score_master.png", 265, 105),
        ("b50_score_remaster.png", 265, 105),
        ("logo.png", 249, 120),
        ("Name.png", 170, 38),
        ("UI_CMN_DXRating_10.png", 220, 50),
        ("UI_CMN_DXRating_11.png", 220, 50),
        ("UI_CMN_Shougou_Rainbow.png", 270, 27),
        ("UI_DNM_DaniPlate_00.png", 120, 48),
        ("UI_FBR_Class_00.png", 110, 70),
        ("UI_Icon_309503.png", 120, 120),
        ("UI_Plate_300501.png", 800, 130),
        ("UI_TTR_Rank_SSS.png", 180, 80),
        ("UI_TTR_Rank_SSSp.png", 180, 80),
        ("SD.png", 70, 30),
        ("DX.png", 70, 30),
        ("UI_TTR_BG_Base_Plus.png", 1400, 780),
        ("UI_CMN_TabTitle_MaimaiTitle_Ver214.png", 380, 180),
        ("UI_CMN_DXRating_S_10.png", 220, 55),
        ("UI_CMN_Name_DX.png", 55, 25),
        ("UI_TST_PlateMask.png", 300, 48),
        ("UI_RSL_MBase_Parts_01.png", 120, 60),
        ("UI_RSL_MBase_Parts_02.png", 120, 60),
        ("UI_GAM_Rank_SSSp.png", 180, 80),
    ] {
        image(&pic.join(name), width, height)?;
    }
    image(&cover.join("00383.png"), 64, 64)?;
    image(&cover.join("10383.png"), 64, 64)?;
    Ok(static_root)
}

fn image(path: &Path, width: u32, height: u32) -> Result<(), image::ImageError> {
    RgbaImage::from_pixel(width, height, Rgba([80, 120, 180, 255]))
        .save_with_format(path, ImageFormat::Png)
}

fn assert_png(payload: &Value) -> TestResult {
    let path = PathBuf::from(
        payload["imagePath"]
            .as_str()
            .ok_or_else(|| io::Error::other("path missing"))?,
    );
    let bytes = fs::read(path)?;
    assert_eq!(bytes.get(..8), Some(b"\x89PNG\r\n\x1a\n".as_slice()));
    let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png)?;
    assert_eq!(
        (u64::from(decoded.width()), u64::from(decoded.height())),
        (
            payload["width"].as_u64().unwrap_or_default(),
            payload["height"].as_u64().unwrap_or_default()
        )
    );
    Ok(())
}

fn content_json(response: &Value) -> TestResult<Value> {
    Ok(serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .ok_or_else(|| io::Error::other("content missing"))?,
    )?)
}

fn assert_error(response: &Value, code: &str) -> TestResult {
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        code
    );
    Ok(())
}

async fn duplex_calls(
    server: crate::ContractServer<B50RenderDispatcher>,
    arguments: Vec<Value>,
) -> TestResult<Vec<Value>> {
    let (client, server_io) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
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
        json!({
        "protocolVersion":"2024-11-05","capabilities":{},
        "clientInfo":{"name":"b50-render-test","version":"1"}}),
    )
    .await?;
    let listed = rpc(&mut write, &mut lines, 2, "tools/list", json!({})).await?;
    assert_eq!(
        listed["result"]["tools"]
            .as_array()
            .ok_or_else(|| io::Error::other("tools missing"))?
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>(),
        [TOOL_NAME]
    );
    let mut output = Vec::new();
    for (index, arguments) in arguments.into_iter().enumerate() {
        output.push(
            rpc(
                &mut write,
                &mut lines,
                index as u64 + 3,
                "tools/call",
                json!({"name":TOOL_NAME,"arguments":arguments}),
            )
            .await?,
        );
    }
    write.shutdown().await?;
    drop(write);
    task.await??;
    Ok(output)
}

async fn rpc<W, R>(
    writer: &mut W,
    lines: &mut Lines<BufReader<R>>,
    id: u64,
    method: &str,
    params: Value,
) -> io::Result<Value>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: tokio::io::AsyncRead + Unpin,
{
    let mut encoded =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .map_err(io::Error::other)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    let line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::other("response missing"))?;
    serde_json::from_str(&line).map_err(io::Error::other)
}
