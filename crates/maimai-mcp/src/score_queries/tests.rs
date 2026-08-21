use std::{error::Error, fs, sync::Arc, time::Duration};

use maimai_app::{
    identity::IdentityDirectory,
    oauth::OAuthService,
    score_service::PlayerScoreService,
    score_settings::{AllowedScoreSources, ScoreSettingsService},
    scores::{
        B50Chart, B50Result, Lookup, PlayerScoreProfile, RatingMode, SelectionReason,
        SourceSelection, compute_fit_index,
    },
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, PlayAchievement, QqId,
    RatingBreakdown, ScoreSource, SongIdNamespace, SourceSongId, UtageScore,
};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_storage::StateStore;
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::{io::AsyncReadExt, net::TcpListener, sync::mpsc};
use url::Url;

use super::{
    MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON, ScoreDeployment, ScoreQueryHandler, TOOL_NAMES,
    convert,
    dto::{B50Args, DivingFishApiArgs},
    filter::DisplayOptions,
    format,
    handler::within,
    score_query_server, serialize,
};
use crate::{ToolCall, ToolDispatcher};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;
type TestValue<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn split_applies_top_n_per_section_and_metadata_flag_hides_fit_text() -> TestResult {
    let result = b50_result()?;
    let split = DisplayOptions::from_args(&B50Args {
        top_n: Some(1),
        section: Some("split".to_owned()),
        ..B50Args::default()
    })?;
    let text = format::b50(&result, None, &split)?;
    assert!(text.contains("Old High") && text.contains("New Low"));
    let hidden = DisplayOptions::from_args(&B50Args {
        include_chart_metadata: Some(false),
        ..B50Args::default()
    })?;
    let hidden_text = format::b50(&result, None, &hidden)?;
    assert!(!hidden_text.contains("拟合") && !hidden_text.contains("虚高指数"));
    Ok(())
}

#[test]
fn sorting_matches_legacy_null_tie_default_and_level_rules() -> TestResult {
    let result = b50_result()?;
    let default = DisplayOptions::from_args(&B50Args {
        sort_order: Some("asc".to_owned()),
        ..B50Args::default()
    })?;
    assert_eq!(
        titles(default.visible(&result)),
        ["Old High", "New Low", "Missing"]
    );
    let ascending = DisplayOptions::from_args(&B50Args {
        sort_by: Some("ra".to_owned()),
        sort_order: Some("asc".to_owned()),
        ..B50Args::default()
    })?;
    assert_eq!(
        titles(ascending.visible(&result)),
        ["New Low", "Old High", "Missing"]
    );
    let level = DisplayOptions::from_args(&B50Args {
        level: Some("14 级?".to_owned()),
        ..B50Args::default()
    })?;
    assert_eq!(titles(level.visible(&result)), ["Old High"]);
    Ok(())
}

#[test]
fn records_text_json_and_achievement_sort_keep_utage_exact_value() -> TestResult {
    let normal = chart("Normal", 300, Some("14.0"), Some("14.0"), false)?;
    let mut utage = chart("Utage", 0, None, None, false)?;
    let song = SourceSongId::numeric(SongIdNamespace::Lxns, 111_597);
    utage.key = ChartKey::new(
        song.clone(),
        ChartGeneration::UtageOnePlayer,
        Difficulty::Utage,
    )?;
    utage.source_song_id = song;
    utage.level = "13+?".to_owned();
    utage.achievements = Some(PlayAchievement::from(UtageScore::from_ten_thousandths(
        1_535_756,
    )));
    utage.rating = None;
    utage.grade = None;
    let result = B50Result {
        lookup: Lookup::Qq(QqId::new("10001")?),
        source: ScoreSource::Local,
        player: PlayerScoreProfile::default(),
        rating_breakdown: RatingBreakdown {
            b35: 300,
            b15: 0,
            total: 300,
        },
        b35: vec![normal, utage.clone()],
        b15: Vec::new(),
        mode: RatingMode::Actual,
        computation: None,
        fit_index: Default::default(),
    };
    let options = DisplayOptions::from_args(&B50Args {
        sort_by: Some("achievement".to_owned()),
        sort_order: Some("desc".to_owned()),
        section: Some("b50".to_owned()),
        ..B50Args::default()
    })?;
    assert_eq!(titles(options.visible(&result))[0], "Utage");
    assert!(format::b50(&result, None, &options)?.contains("153.5756%"));
    let serialized = serialize::b50(
        &result,
        serialize::OutputContext {
            requested_at: "2026-08-19T00:00:00Z".to_owned(),
            identity: None,
            selection: SourceSelection {
                preferred_source: ScoreSource::Local,
                source: ScoreSource::Local,
                reason: SelectionReason::ExplicitOverride,
            },
            include_chart_metadata: false,
            raw: None,
        },
    )?;
    assert_eq!(
        serialized["charts"]["sd"][1]["achievements"],
        json!(153.5756)
    );
    Ok(())
}

#[test]
fn serializer_keeps_preferred_and_used_source_and_fit_precision() -> TestResult {
    let result = b50_result()?;
    let output = serialize::b50(
        &result,
        serialize::OutputContext {
            requested_at: "2026-08-18T00:00:00Z".to_owned(),
            identity: None,
            selection: SourceSelection {
                preferred_source: ScoreSource::Local,
                source: ScoreSource::DivingFish,
                reason: SelectionReason::ExplicitOverride,
            },
            include_chart_metadata: true,
            raw: None,
        },
    )?;
    assert_eq!(output["sourcePreference"]["preferredSource"], "local");
    assert_eq!(output["sourcePreference"]["usedSource"], "sy");
    assert_eq!(output["sourcePreference"]["explicitSource"], true);
    assert!(output["fitIndex"]["b50"]["virtualRatio"].is_number());
    Ok(())
}

#[test]
fn secret_inputs_never_enter_conversion_errors() -> TestResult {
    let secret = "developer-secret-sentinel";
    let args: DivingFishApiArgs = serde_json::from_value(json!({
        "operation":"unknown","developerToken":secret,
        "importToken":"import-secret-sentinel","jwtToken":"jwt-secret-sentinel"
    }))?;
    let error = convert::generic_request(args, None)
        .err()
        .ok_or("expected invalid operation")?;
    let rendered = error.structured().to_string();
    for forbidden in [secret, "import-secret-sentinel", "jwt-secret-sentinel"] {
        assert!(!rendered.contains(forbidden));
    }
    Ok(())
}

#[tokio::test]
async fn whole_query_timeout_returns_typed_tool_error() -> TestResult {
    let error = within(Duration::from_millis(5), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok::<_, super::error::ScoreQueryToolError>(())
    })
    .await
    .err()
    .ok_or("expected timeout")?;
    assert_eq!(error.structured()["code"], "TIMEOUT");
    Ok(())
}

#[tokio::test]
async fn song_query_resolves_unique_diving_fish_id_not_catalog_primary_id() -> TestResult {
    let temp = TempDir::new()?;
    write_catalog(&temp)?;
    let store = CatalogStore::load(CatalogFiles::from_data_dir(temp.path())).await?;
    let filter = convert::song_filter(
        None,
        Some("Link".to_owned()),
        None,
        None,
        None,
        &store.snapshot(),
    )?;
    assert_eq!(filter.song.namespace(), SongIdNamespace::DivingFish);
    assert_eq!(filter.song.value(), &maimai_core::SongIdValue::Numeric(383));
    Ok(())
}

#[tokio::test]
async fn duplex_lists_exact_frozen_main_and_public_query_surfaces() -> TestResult {
    let (main, _main_temp) = handler(ScoreDeployment::Main).await?;
    let (public, _public_temp) = handler(ScoreDeployment::Public).await?;
    for (contract, handler) in [(MAIN_CONTRACT_JSON, main), (PUBLIC_CONTRACT_JSON, public)] {
        let server = score_query_server(contract, handler)?;
        assert_eq!(server.tools().len(), TOOL_NAMES.len());
        let listed = duplex_tool_names(server).await?;
        assert_eq!(
            listed,
            TOOL_NAMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

#[tokio::test]
async fn local_handler_returns_text_and_structured_without_network() -> TestResult {
    let (handler, _temp) = handler(ScoreDeployment::Main).await?;
    let arguments = json!({"qq":"10001","source":"local","includeChartMetadata":false})
        .as_object()
        .cloned()
        .ok_or("arguments object missing")?;
    let output = handler
        .dispatch(ToolCall::new("query_b50".to_owned(), arguments))
        .await?;
    let (content, structured) = output.into_parts();
    let encoded = serde_json::to_value(content)?;
    let text = encoded[0]["text"].as_str().ok_or("text missing")?;
    assert!(text.contains("Local Tester") && !text.contains("拟合"));
    assert_eq!(
        structured.ok_or("structured missing")?["source"],
        "local-maimai-db"
    );
    Ok(())
}

#[tokio::test]
async fn local_records_apply_typed_plate_membership_and_return_metadata() -> TestResult {
    let (handler, _temp) = handler(ScoreDeployment::Main).await?;
    let arguments = json!({"qq":"10001","source":"local","plate":"熊","server":"cn"})
        .as_object()
        .cloned()
        .ok_or("arguments object missing")?;
    let output = handler
        .dispatch(ToolCall::new(
            "query_maimai_player_records".to_owned(),
            arguments,
        ))
        .await?;
    let (_, structured) = output.into_parts();
    let structured = structured.ok_or("structured missing")?;
    assert_eq!(structured["counts"]["filtered"], 1);
    assert_eq!(
        structured["plate"],
        json!({"name":"熊","server":"cn","songCount":1})
    );
    Ok(())
}

#[tokio::test]
async fn batch_forces_diving_fish_keeps_order_and_continuously_refills_concurrency() -> TestResult {
    let (base, mut events) = mock_http(vec![50, 200, 0]).await?;
    let (handler, store, _temp) = handler_at(ScoreDeployment::Main, base).await?;
    for qq in ["10001", "10002", "10003"] {
        store
            .set_score_source_preference(&QqId::new(qq)?, ScoreSource::Local)
            .await?;
    }
    let arguments = json!({"qqs":["10001","10002","10003"],"maxConcurrency":2,
        "queryDelayMs":0,"includeChartMetadata":false})
    .as_object()
    .cloned()
    .ok_or("arguments object missing")?;
    let output = handler
        .dispatch(ToolCall::new("query_b50_batch".to_owned(), arguments))
        .await?;
    let (_, structured) = output.into_parts();
    let structured = structured.ok_or("structured missing")?;
    assert_eq!(
        structured["counts"],
        json!({"requested":3,"success":3,"failure":0}),
        "{}",
        structured["results"]
    );
    assert_eq!(
        structured["results"]
            .as_array()
            .ok_or("results missing")?
            .iter()
            .map(|item| item["qq"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        ["10001", "10002", "10003"]
    );
    assert!(
        structured["results"]
            .as_array()
            .ok_or("results missing")?
            .iter()
            .all(|item| item["result"]["source"] == "diving-fish")
    );
    let mut observed = Vec::new();
    for _ in 0..6 {
        observed.push(events.recv().await.ok_or("event stream closed")?);
    }
    let accepted_third = observed
        .iter()
        .position(|event| event == "accepted:2")
        .ok_or("third accept missing")?;
    let completed_second = observed
        .iter()
        .position(|event| event == "completed:1")
        .ok_or("second completion missing")?;
    assert!(accepted_third < completed_second, "{observed:?}");
    Ok(())
}

#[tokio::test]
async fn generic_api_executes_through_injected_provider_and_returns_safe_shape() -> TestResult {
    let (base, mut events) = mock_http(vec![0]).await?;
    let (handler, _, _temp) = handler_at(ScoreDeployment::Main, base).await?;
    let arguments = json!({"operation":"public_alive_check_get","includeHeaders":true})
        .as_object()
        .cloned()
        .ok_or("arguments object missing")?;
    let output = handler
        .dispatch(ToolCall::new("diving_fish_api".to_owned(), arguments))
        .await?;
    let (_, structured) = output.into_parts();
    let structured = structured.ok_or("structured missing")?;
    assert_eq!(structured["operation"], "public_alive_check_get");
    assert_eq!(structured["status"], 200);
    assert_eq!(structured["data"]["nickname"], "DF Tester");
    assert_eq!(events.recv().await.as_deref(), Some("accepted:0"));
    Ok(())
}

fn b50_result() -> TestValue<B50Result> {
    let mut old = chart("Old High", 300, Some("14.0"), Some("13.7"), false)?;
    old.achievements = Some(AchievementRate::from_decimal_str("100.6000")?.into());
    let new = chart("New Low", 200, Some("13.5"), Some("13.7"), true)?;
    let missing = chart("Missing", 0, None, None, false)?;
    let b35 = vec![old, missing];
    let b15 = vec![new];
    Ok(B50Result {
        lookup: Lookup::Qq(QqId::new("10001")?),
        source: ScoreSource::DivingFish,
        player: PlayerScoreProfile {
            nickname: Some("Tester".to_owned()),
            rating: Some(500),
            ..PlayerScoreProfile::default()
        },
        rating_breakdown: RatingBreakdown {
            b35: 300,
            b15: 200,
            total: 500,
        },
        fit_index: compute_fit_index(&b35, &b15),
        b35,
        b15,
        mode: RatingMode::Actual,
        computation: None,
    })
}

fn chart(
    title: &str,
    rating: u32,
    constant: Option<&str>,
    fit: Option<&str>,
    current: bool,
) -> TestValue<B50Chart> {
    let song = SourceSongId::numeric(SongIdNamespace::DivingFish, rating.max(1));
    Ok(B50Chart {
        key: ChartKey::new(
            song.clone(),
            if current {
                ChartGeneration::Deluxe
            } else {
                ChartGeneration::Standard
            },
            Difficulty::Master,
        )?,
        source_song_id: song,
        title: title.to_owned(),
        level: if title == "Old High" { "14" } else { "13+" }.to_owned(),
        constant: constant.map(ChartConstant::from_decimal_str).transpose()?,
        achievements: Some(AchievementRate::from_decimal_str("100.0000")?.into()),
        dx_score: Some(1_000),
        rating: (rating > 0).then_some(rating),
        original_rating: None,
        grade: Some("sss".to_owned()),
        full_combo: None,
        full_sync: None,
        version: if current { "Current" } else { "Old" }.to_owned(),
        is_current: current,
        fit_constant: fit.map(ChartConstant::from_decimal_str).transpose()?,
        fit_label: None,
    })
}

fn titles(values: Vec<&B50Chart>) -> Vec<&str> {
    values
        .into_iter()
        .map(|chart| chart.title.as_str())
        .collect()
}

async fn handler(deployment: ScoreDeployment) -> TestValue<(ScoreQueryHandler, TempDir)> {
    let (handler, _, temp) = handler_at(deployment, Url::parse("http://127.0.0.1:9/")?).await?;
    Ok((handler, temp))
}

async fn handler_at(
    deployment: ScoreDeployment,
    base: Url,
) -> TestValue<(ScoreQueryHandler, StateStore, TempDir)> {
    let temp = TempDir::new()?;
    write_catalog(&temp)?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    seed_local(&store).await?;
    let catalog = Arc::new(CatalogStore::load(CatalogFiles::from_data_dir(temp.path())).await?);
    let df = DivingFishClient::with_base_urls(
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
    let service = Arc::new(PlayerScoreService::with_lxns(
        store.clone(),
        Arc::clone(&catalog),
        DivingFishScoreClient::new(df.clone()),
        oauth,
        LxnsScoreEndpoint::new(base.join("lxns/")?, Duration::from_secs(1))?,
    ));
    let handler = ScoreQueryHandler::new(
        service,
        IdentityDirectory::new(store.clone()),
        ScoreSettingsService::new(
            store.clone(),
            if deployment == ScoreDeployment::Public {
                AllowedScoreSources::public()
            } else {
                AllowedScoreSources::main()
            },
        ),
        catalog,
        df,
        deployment,
    );
    Ok((handler, store, temp))
}

async fn mock_http(delays_ms: Vec<u64>) -> TestValue<(Url, mpsc::Receiver<String>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(delays_ms.len().saturating_mul(2).max(1));
    tokio::spawn(async move {
        for (index, delay) in delays_ms.into_iter().enumerate() {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let sender = sender.clone();
            tokio::spawn(async move {
                let _ = read_http_request(&mut stream).await;
                let _ = sender.send(format!("accepted:{index}")).await;
                tokio::time::sleep(Duration::from_millis(delay)).await;
                let body = json!({"nickname":"DF Tester","rating":500,"charts":{"sd":[{
                    "song_id":383,"title":"Link","type":"SD","level":"13","level_index":3,
                    "ds":"13.0","achievements":"100.0000","ra":280}],"dx":[]}})
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = sender.send(format!("completed:{index}")).await;
            });
        }
    });
    Ok((Url::parse(&format!("http://{address}/"))?, receiver))
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Result<(), std::io::Error> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_start) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_start + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")?
                    .trim()
                    .parse::<usize>()
                    .ok()
            })
            .unwrap_or(0);
        if request.len() >= header_end + length {
            return Ok(());
        }
    }
}

async fn duplex_tool_names(
    server: crate::ContractServer<ScoreQueryHandler>,
) -> TestValue<Vec<String>> {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        let running = rmcp::serve_server(server, server_io)
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        match running
            .waiting()
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?
        {
            QuitReason::JoinError(error) => Err(std::io::Error::other(error.to_string())),
            _ => Ok(()),
        }
    });
    let (read, mut write) = tokio::io::split(client_io);
    let mut lines = BufReader::new(read).lines();
    write_message(
        &mut write,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"score-query-test","version":"1"}}}),
    )
    .await?;
    let _ = read_message(&mut lines).await?;
    write_message(
        &mut write,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await?;
    let response = read_message(&mut lines).await?;
    let names = response["result"]["tools"]
        .as_array()
        .ok_or("tools missing")?
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect();
    drop(lines);
    drop(write);
    task.await??;
    Ok(names)
}

async fn write_message(
    writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    value: Value,
) -> Result<(), std::io::Error> {
    writer
        .write_all(serde_json::to_string(&value)?.as_bytes())
        .await?;
    writer.write_all(b"\n").await
}

async fn read_message(
    lines: &mut tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
) -> TestValue<Value> {
    let line = lines.next_line().await?.ok_or("server closed")?;
    Ok(serde_json::from_str(&line)?)
}

fn write_catalog(temp: &TempDir) -> Result<(), std::io::Error> {
    let files = [
        (
            "lxns_song_list.json",
            json!({"songs":[{"id":383,"title":"Link","artist":"A","genre":"maimai",
                "bpm":150,"version":25000,"difficulties":{"standard":[{
                "difficulty":3,"level":"13","level_value":13.0,"notes":{}}]}}],
                "genres":[],"versions":[{"title":"Current","version":25000}]})
            .to_string(),
        ),
        (
            "divingfish_song_list.json",
            json!([{"id":"383","title":"Link","type":"SD","ds":[1,2,3,13.0],
                "level":["1","2","3","13"],"charts":[{},{},{},{}],
                "basic_info":{"bpm":150,"from":"Current","is_new":true}}])
            .to_string(),
        ),
        ("lxns_alias_list.json", "{\"aliases\":[]}".to_owned()),
        ("music_alias.json", "{\"content\":[]}".to_owned()),
        ("custom_aliases.json", "{}".to_owned()),
        ("pinyin_aliases.json", "{\"aliases\":[]}".to_owned()),
        ("zh_s2t.json", "{}".to_owned()),
        (
            "maimaidxplate.json",
            "{\"content\":{\"熊&华\":[383]}}".to_owned(),
        ),
    ];
    for (name, content) in files {
        fs::write(temp.path().join(name), content)?;
    }
    Ok(())
}

async fn seed_local(store: &StateStore) -> TestResult {
    use maimai_storage::{PlayerProfile, PlayerRecord};
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
            dx_score: Some(1000),
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
