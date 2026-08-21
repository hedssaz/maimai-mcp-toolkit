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
        B50ImageDataService, B50ImageService, B50ImageStyle, OutputPolicy, OutputStore,
        ResourceDirectories, StyleStore,
    },
    identity::IdentityDirectory,
    oauth::OAuthService,
    score_service::PlayerScoreService,
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, GroupId, QqId,
    ScoreSource, SongIdNamespace, SourceSongId,
};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_render::{LegacyAssets, LegacyRenderer};
use maimai_storage::{
    IdentityGroupSnapshot, IdentitySnapshot, IdentitySnapshotMember, PlayerProfile, PlayerRecord,
    StateStore,
};
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::mpsc,
    time::timeout,
};
use url::Url;

use super::{B50ImageDispatcher, b50_image_server};
use crate::{DispatchError, ToolCall};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone)]
struct MockResponse {
    status: u16,
    delay: Duration,
    body: String,
}

struct Fixture {
    _temp: TempDir,
    dispatcher: B50ImageDispatcher,
    data: Arc<B50ImageDataService>,
    store: StateStore,
}

#[tokio::test]
async fn qq_preference_username_and_b50_data_keep_query_boundaries() -> TestResult {
    let (base, mut requests) = mock_http(vec![success()]).await?;
    let fixture = fixture(base).await?;
    seed_local(&fixture.store).await?;
    fixture
        .store
        .set_score_source_preference(&QqId::new("10001")?, ScoreSource::Local)
        .await?;

    let local = dispatch(&fixture.dispatcher, json!({"qq":"10001","style":"legacy"})).await?;
    assert_eq!(local["player"]["nickname"], "Local Tester");
    assert!(local["caption"].as_str().is_some_and(|value| {
        value.starts_with("成绩更新时间：2026-01-15 08:00:00\n当前数据源：本地缓存")
    }));
    assert!(
        timeout(Duration::from_millis(50), requests.recv())
            .await
            .is_err()
    );

    let provided = dispatch(
        &fixture.dispatcher,
        json!({"b50Data":fake_b50(),"style":"legacy"}),
    )
    .await?;
    assert_eq!(provided["lookup"]["qq"], "provided");
    assert!(
        timeout(Duration::from_millis(50), requests.recv())
            .await
            .is_err()
    );

    let username = dispatch(
        &fixture.dispatcher,
        json!({"username":"waterfish-user","style":"yuzu"}),
    )
    .await?;
    assert_eq!(username["style"], "yuzu");
    assert_eq!(username["lookup"], json!({"username":"waterfish-user"}));
    assert_eq!(
        requests.recv().await.ok_or("provider request missing")?,
        json!({"username":"waterfish-user","b50":"1"})
    );
    Ok(())
}

#[tokio::test]
async fn target_unique_fallback_ambiguity_and_group_scope_match_legacy() -> TestResult {
    let (base, mut requests) = mock_http(vec![success(), success(), success()]).await?;
    let fixture = fixture(base).await?;
    seed_identities(&fixture.store).await?;

    let unique = dispatch(
        &fixture.dispatcher,
        json!({"target":"Captain","groupId":"20001","style":"maibot"}),
    )
    .await?;
    assert_eq!(unique["style"], "maibot");
    assert_eq!(unique["lookup"], json!({"qq":"10001"}));
    assert_eq!(
        requests.recv().await.ok_or("unique request missing")?,
        json!({"qq":"10001","b50":"1"})
    );

    let fallback = dispatch(
        &fixture.dispatcher,
        json!({"target":"unknown-user","groupId":"20001","style":"legacy"}),
    )
    .await?;
    assert_eq!(fallback["lookup"], json!({"username":"unknown-user"}));
    assert_eq!(
        requests.recv().await.ok_or("fallback request missing")?,
        json!({"username":"unknown-user","b50":"1"})
    );

    let ambiguous = dispatch_error(
        &fixture.dispatcher,
        json!({"target":"Same","groupId":"20001","style":"legacy"}),
    )
    .await?;
    assert_eq!(ambiguous["error"]["code"], "AMBIGUOUS_IDENTITY");
    assert!(
        ambiguous["error"]["body"]
            .as_str()
            .is_some_and(|body| body.contains("10002") && body.contains("10003"))
    );

    let scoped = dispatch(
        &fixture.dispatcher,
        json!({"target":"Same","groupId":"20002","style":"legacy"}),
    )
    .await?;
    assert_eq!(scoped["lookup"], json!({"qq":"10004"}));
    assert_eq!(
        requests.recv().await.ok_or("scoped request missing")?,
        json!({"qq":"10004","b50":"1"})
    );
    Ok(())
}

#[tokio::test]
async fn query_timeout_is_real_and_provider_errors_are_redacted() -> TestResult {
    let secret = "B50_PROVIDER_SECRET";
    let (base, mut requests) = mock_http(vec![
        MockResponse {
            delay: Duration::from_millis(1_200),
            ..success()
        },
        MockResponse {
            status: 500,
            delay: Duration::ZERO,
            body: json!({"token":secret,"message":"failure"}).to_string(),
        },
    ])
    .await?;
    let fixture = fixture(base).await?;

    let timed_out = dispatch_error(
        &fixture.dispatcher,
        json!({"qq":"20001","style":"legacy","timeoutMs":1000}),
    )
    .await?;
    assert_eq!(timed_out["error"]["code"], "TIMEOUT");
    assert_eq!(
        requests.recv().await.ok_or("slow request missing")?,
        json!({"qq":"20001","b50":"1"})
    );
    assert!(
        fixture
            .store
            .player_b50_snapshot(&QqId::new("20001")?, OffsetDateTime::UNIX_EPOCH)
            .await?
            .is_none()
    );

    let dispatcher = fixture.dispatcher.clone();
    let provider_task = tokio::spawn(async move {
        dispatch_error(&dispatcher, json!({"username":"failure","style":"legacy"})).await
    });
    assert!(
        timeout(Duration::from_millis(100), requests.recv())
            .await
            .is_err()
    );
    assert_eq!(
        requests.recv().await.ok_or("failure request missing")?,
        json!({"username":"failure","b50":"1"})
    );
    let provider = provider_task.await??;
    assert_eq!(provider["error"]["code"], "PROVIDER_ERROR");
    assert!(!provider.to_string().contains(secret));
    fixture.data.wait_idle().await?;
    let snapshot = fixture
        .store
        .player_b50_snapshot(&QqId::new("20001")?, OffsetDateTime::UNIX_EPOCH)
        .await?
        .ok_or("completed snapshot missing")?;
    assert_eq!(snapshot.charts().len(), 1);
    Ok(())
}

#[tokio::test]
async fn stdio_provider_success_is_not_error_and_omits_raw_secrets() -> TestResult {
    let secret = "B50_STDIO_SECRET";
    let mut response = success();
    let mut body: Value = serde_json::from_str(&response.body)?;
    body["token"] = json!(secret);
    response.body = body.to_string();
    let (base, _) = mock_http(vec![response]).await?;
    let fixture = fixture(base).await?;
    let server = b50_image_server(fixture.dispatcher)?;
    let (client, server_io) = tokio::io::duplex(2 * 1024 * 1024);
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
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"fixture","version":"1"}}}),
    )
    .await?;
    write_rpc(
        &mut write,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await?;
    let called = rpc(
        &mut write,
        &mut lines,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"render_b50_image","arguments":{"username":"stdio","style":"legacy"}}}),
    )
    .await?;
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(
        called["result"]["structuredContent"]["mimeType"],
        "image/png"
    );
    assert!(!called.to_string().contains(secret));
    assert!(called["result"]["structuredContent"].get("raw").is_none());
    write.shutdown().await?;
    drop(lines);
    server_task.await??;
    Ok(())
}

async fn fixture(base: Url) -> TestResult<Fixture> {
    let temp = TempDir::new()?;
    write_catalog(temp.path())?;
    let static_root = create_assets(temp.path())?;
    let cover_root = temp.path().join("cover-cache");
    fs::create_dir(&cover_root)?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
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
        store.clone(),
        catalog,
        DivingFishScoreClient::new(diving_fish),
        oauth,
        LxnsScoreEndpoint::new(base.join("lxns/")?, Duration::from_secs(1))?,
    ));
    let data = Arc::new(B50ImageDataService::with_max_query_concurrency(
        scores,
        IdentityDirectory::new(store.clone()),
        1,
    ));
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf");
    let images = B50ImageService::new(
        StyleStore::new(temp.path().join("style.json"), B50ImageStyle::Yuzu)?,
        OutputStore::new(temp.path().join("images"), OutputPolicy::standard())?,
        LegacyRenderer::new(LegacyAssets::new(&font, &font))?,
        ResourceDirectories::new(&static_root, &cover_root)?,
    );
    Ok(Fixture {
        _temp: temp,
        dispatcher: B50ImageDispatcher::new(images, Arc::clone(&data)),
        data,
        store,
    })
}

async fn dispatch(dispatcher: &B50ImageDispatcher, args: Value) -> TestResult<Value> {
    let output = dispatcher
        .dispatch_call_at(call(args)?, fixed_now()?)
        .await?;
    output
        .into_parts()
        .1
        .ok_or_else(|| io::Error::other("structured content missing").into())
}

async fn dispatch_error(dispatcher: &B50ImageDispatcher, args: Value) -> TestResult<Value> {
    let error = dispatcher
        .dispatch_call_at(call(args)?, fixed_now()?)
        .await
        .err()
        .ok_or_else(|| io::Error::other("expected tool error"))?;
    let DispatchError::Tool(failure) = error else {
        return Err(io::Error::other("expected tool failure").into());
    };
    failure
        .into_parts()
        .1
        .ok_or_else(|| io::Error::other("structured error missing").into())
}

fn call(args: Value) -> TestResult<ToolCall> {
    Ok(ToolCall::new(
        "render_b50_image".to_owned(),
        args.as_object()
            .cloned()
            .ok_or_else(|| io::Error::other("arguments missing"))?,
    ))
}

fn fixed_now() -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp(1_768_435_200)
}

async fn mock_http(responses: Vec<MockResponse>) -> TestResult<(Url, mpsc::Receiver<Value>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(responses.len().max(1));
    tokio::spawn(async move {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let sender = sender.clone();
            tokio::spawn(async move {
                if let Ok(body) = read_request(&mut stream).await {
                    let _ = sender.send(body).await;
                }
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
    Ok((Url::parse(&format!("http://{address}/"))?, receiver))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> io::Result<Value> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::other("request closed"));
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_start) = request.windows(4).position(|value| value == b"\r\n\r\n") else {
            continue;
        };
        let body_start = header_start + 4;
        let headers = String::from_utf8_lossy(&request[..body_start]);
        let length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")?
                    .trim()
                    .parse()
                    .ok()
            })
            .unwrap_or(0);
        if request.len() >= body_start + length {
            return serde_json::from_slice(&request[body_start..body_start + length])
                .map_err(io::Error::other);
        }
    }
}

fn success() -> MockResponse {
    MockResponse {
        status: 200,
        delay: Duration::ZERO,
        body: json!({"nickname":"DF Tester","rating":280,"charts":{"sd":[{
            "song_id":383,"title":"Link","type":"SD","level":"13","level_index":3,
            "ds":"13.0","achievements":"100.0000","ra":280,"rate":"sss"
        }],"dx":[]}})
        .to_string(),
    }
}

fn fake_b50() -> Value {
    json!({"lookup":{"qq":"provided"},"player":{"nickname":"Provided","rating":0},
        "counts":{"sd":0,"dx":0,"total":0},"ratingBreakdown":{"sd":0,"dx":0,"total":0},
        "charts":{"sd":[],"dx":[]}})
}

async fn seed_local(store: &StateStore) -> TestResult {
    let qq = QqId::new("10001")?;
    store
        .upsert_profile(&PlayerProfile {
            qq: qq.clone(),
            nickname: Some("Local Tester".to_owned()),
            player_rating: Some(280),
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
                SourceSongId::numeric(SongIdNamespace::Lxns, 383),
                ChartGeneration::Standard,
                Difficulty::Master,
            )?,
            title: "Link".to_owned(),
            level: Some("13".to_owned()),
            level_label: Some("Master".to_owned()),
            ds: Some(ChartConstant::from_decimal_str("13.0")?),
            achievements: Some(AchievementRate::from_decimal_str("100.0000")?.into()),
            dx_score: Some(1_000),
            fc: None,
            fs: None,
            rate: Some("sss".to_owned()),
            ra: Some(280),
            version: Some("Current".to_owned()),
            is_new: true,
            score_source: ScoreSource::Local,
            source_detail: None,
            raw: None,
            payload: json!({}),
            updated_at: "2026-08-18T00:00:00Z".to_owned(),
        })
        .await?;
    Ok(())
}

async fn seed_identities(store: &StateStore) -> TestResult {
    store
        .replace_identity_snapshot(&IdentitySnapshot {
            fetched_at: fixed_now()?,
            friends: Vec::new(),
            groups: vec![
                group(
                    "20001",
                    &[("10001", "Captain"), ("10002", "Same"), ("10003", "Same")],
                )?,
                group("20002", &[("10004", "Same")])?,
            ],
        })
        .await?;
    Ok(())
}

fn group(id: &str, members: &[(&str, &str)]) -> TestResult<IdentityGroupSnapshot> {
    Ok(IdentityGroupSnapshot {
        group_id: GroupId::new(id)?,
        group_name: Some(format!("Group {id}")),
        member_count: Some(members.len() as u64),
        members: members
            .iter()
            .map(|(qq, card)| {
                Ok(IdentitySnapshotMember {
                    qq: QqId::new(*qq)?,
                    nickname: Some(format!("User {qq}")),
                    card: Some((*card).to_owned()),
                })
            })
            .collect::<TestResult<Vec<_>>>()?,
    })
}

fn write_catalog(root: &Path) -> io::Result<()> {
    let files = [
        (
            "divingfish_song_list.json",
            json!([{"id":"383","title":"Link","type":"SD",
            "ds":[1,2,3,13.0],"level":["1","2","3","13"],"charts":[{},{},{},{}],
            "basic_info":{"bpm":150,"from":"Current","is_new":true}}])
            .to_string(),
        ),
        (
            "lxns_song_list.json",
            json!({"songs":[{"id":383,"title":"Link","artist":"A",
            "genre":"maimai","bpm":150,"version":25000,"difficulties":{"standard":[{
            "difficulty":3,"level":"13","level_value":13.0,"notes":{}}]}}],"genres":[],
            "versions":[{"title":"Current","version":25000}]})
            .to_string(),
        ),
        ("lxns_alias_list.json", r#"{"aliases":[]}"#.to_owned()),
        ("music_alias.json", r#"{"content":[]}"#.to_owned()),
        ("custom_aliases.json", "{}".to_owned()),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#.to_owned()),
        ("zh_s2t.json", "{}".to_owned()),
    ];
    for (name, content) in files {
        fs::write(root.join(name), content)?;
    }
    Ok(())
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
        ("UI_GAM_Rank_SSS.png", 180, 80),
        ("UI_GAM_Rank_SSSp.png", 180, 80),
    ] {
        image(&pic.join(name), width, height)?;
    }
    image(&cover.join("00383.png"), 64, 64)?;
    Ok(static_root)
}

fn image(path: &Path, width: u32, height: u32) -> Result<(), image::ImageError> {
    RgbaImage::from_pixel(width, height, Rgba([80, 120, 180, 255]))
        .save_with_format(path, ImageFormat::Png)
}

async fn write_rpc<W>(writer: &mut W, value: &Value) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut encoded = serde_json::to_vec(value).map_err(io::Error::other)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await
}

async fn rpc<W, R>(
    writer: &mut W,
    lines: &mut tokio::io::Lines<BufReader<R>>,
    value: Value,
) -> io::Result<Value>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: tokio::io::AsyncRead + Unpin,
{
    write_rpc(writer, &value).await?;
    let line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::other("RPC response missing"))?;
    serde_json::from_str(&line).map_err(io::Error::other)
}
