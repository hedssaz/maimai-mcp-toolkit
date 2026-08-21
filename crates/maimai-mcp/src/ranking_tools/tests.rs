use std::{error::Error, fs, io, sync::Arc, time::Duration};

use maimai_app::{
    identity::IdentityService, oauth::OAuthService, rankings::RankingService,
    score_service::PlayerScoreService,
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_core::{
    AchievementRate, ChartGeneration, ChartKey, Difficulty, GroupId, QqId, RatingBreakdown,
    SongIdNamespace, SourceSongId,
};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, NapCatClient, NapCatConfig,
    OAuthConfig, lxns_score::LxnsScoreEndpoint,
};
use maimai_storage::{
    CachedB50Entry, CachedChart, CachedFitIndex, CachedPlayer, RankingJobStart, RankingMember,
    RankingNamespace, RankingRefreshReason, RankingSnapshot, StateStore,
};
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::{OffsetDateTime, UtcOffset};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use url::Url;

use super::{MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON, RankingDispatcher, rankings_server};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn duplex_lists_all_tools_and_cache_status_has_no_path_leak() -> TestResult {
    let fixture = fixture().await?;
    let group = GroupId::new("123")?;
    let fetched = OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1);
    let job = fixture
        .store
        .start_ranking_job(
            RankingNamespace::B50,
            &group,
            RankingRefreshReason::Miss,
            fetched,
        )
        .await?;
    let RankingJobStart::Started(job) = job else {
        return Err("ranking job did not start".into());
    };
    let member = member()?;
    let chart = chart()?;
    fixture
        .store
        .complete_b50_ranking(
            &RankingSnapshot {
                namespace: RankingNamespace::B50,
                group_id: group.clone(),
                generation: job.generation,
                fetched_at: fetched,
                next_reset_at: fetched + time::Duration::days(1),
                member_count: 1,
                success_count: 1,
                failure_count: 0,
                skipped_count: 0,
                cache_hit_count: 0,
                shared_fetch_count: 0,
            },
            &[CachedB50Entry {
                member: member.clone(),
                player: CachedPlayer {
                    nickname: Some("player".to_owned()),
                    rating: Some(15000),
                    ..CachedPlayer::default()
                },
                rating_breakdown: RatingBreakdown {
                    b35: 10000,
                    b15: 5000,
                    total: 15000,
                },
                fit_index: CachedFitIndex::default(),
                charts: Vec::new(),
            }],
            fetched,
        )
        .await?;
    let song_job = fixture
        .store
        .start_ranking_job(
            RankingNamespace::SongScore,
            &group,
            RankingRefreshReason::Miss,
            fetched,
        )
        .await?;
    let RankingJobStart::Started(song_job) = song_job else {
        return Err("song ranking job did not start".into());
    };
    fixture
        .store
        .complete_song_ranking(
            &RankingSnapshot {
                namespace: RankingNamespace::SongScore,
                group_id: group.clone(),
                generation: song_job.generation,
                fetched_at: fetched,
                next_reset_at: fetched + time::Duration::days(1),
                member_count: 1,
                success_count: 1,
                failure_count: 0,
                skipped_count: 0,
                cache_hit_count: 0,
                shared_fetch_count: 0,
            },
            std::slice::from_ref(&member),
            &[(member.qq.clone(), chart)],
            fetched,
        )
        .await?;

    let dispatcher = RankingDispatcher::new(fixture.service, UtcOffset::from_hms(9, 0, 0)?);
    let server = rankings_server(MAIN_CONTRACT_JSON, dispatcher)?;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
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
    let (read, mut write) = tokio::io::split(client_io);
    let mut lines = BufReader::new(read).lines();
    send(
        &mut write,
        json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2024-11-05","capabilities":{},
                "clientInfo":{"name":"ranking-test","version":"1"}}
        }),
    )
    .await?;
    let initialized = receive(&mut lines).await?;
    assert_eq!(
        initialized["result"]["serverInfo"]["name"],
        "group-rank-mcp"
    );
    send(
        &mut write,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await?;
    send(
        &mut write,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await?;
    let tools = receive(&mut lines).await?;
    assert_eq!(tools["result"]["tools"].as_array().map(Vec::len), Some(11));
    send(
        &mut write,
        json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"group_b50_cache_status","arguments":{"groupId":"123"}}
        }),
    )
    .await?;
    let called = receive(&mut lines).await?;
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(called["result"]["structuredContent"]["cacheExists"], true);
    assert!(
        called["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("1970-01-01 10:00:00 +09:00"))
    );
    let rendered = called.to_string();
    assert!(!rendered.contains("cachePath"));
    assert!(!rendered.contains("cacheDir"));
    assert!(!rendered.contains("reportFiles"));

    let calls = [
        (4, "group_b50_report", json!({"groupId":"123"})),
        (5, "group_b50_job_status", json!({"groupId":"123"})),
        (
            6,
            "group_b50_member_rank",
            json!({"groupId":"123","qq":"10001"}),
        ),
        (7, "group_b50_rank_at", json!({"groupId":"123","rank":1})),
        (
            8,
            "group_song_score_report",
            json!({"groupId":"123","musicId":383}),
        ),
        (
            9,
            "group_song_score_member_rank",
            json!({"groupId":"123","qq":"10001","musicId":383}),
        ),
        (
            10,
            "group_song_score_cache_status",
            json!({"groupId":"123"}),
        ),
        (11, "group_song_score_job_status", json!({"groupId":"123"})),
        (12, "clear_group_b50_cache", json!({"groupId":"123"})),
        (13, "clear_group_song_score_cache", json!({"groupId":"123"})),
    ];
    for (id, name, arguments) in calls {
        send(
            &mut write,
            json!({
                "jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":name,"arguments":arguments}
            }),
        )
        .await?;
        let response = receive(&mut lines).await?;
        assert_eq!(response["result"]["isError"], false, "{name}: {response}");
        let rendered = response.to_string();
        assert!(!rendered.contains("cachePath"), "{name}");
        assert!(!rendered.contains("reportFiles"), "{name}");
    }
    send(
        &mut write,
        json!({
            "jsonrpc":"2.0","id":14,"method":"tools/call",
            "params":{"name":"group_b50_report","arguments":{
                "groupId":"999","forceRefresh":true,
                "napcatBaseUrl":"http://secret-sentinel@127.0.0.1:9/"
            }}
        }),
    )
    .await?;
    let failed = receive(&mut lines).await?;
    assert_eq!(failed["result"]["isError"], true);
    assert_eq!(
        failed["result"]["structuredContent"]["error"]["code"],
        "INVALID_INPUT"
    );
    assert!(!failed.to_string().contains("secret-sentinel"));
    write.shutdown().await?;
    drop(write);
    task.await??;
    Ok(())
}

#[test]
fn main_and_public_freeze_the_same_eleven_tool_names() -> TestResult {
    let main: Value = serde_json::from_str(MAIN_CONTRACT_JSON)?;
    let public: Value = serde_json::from_str(PUBLIC_CONTRACT_JSON)?;
    let names = |value: &Value| {
        value["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&main), names(&public));
    assert_eq!(names(&main).len(), 11);
    Ok(())
}

struct Fixture {
    _temp: TempDir,
    store: StateStore,
    service: RankingService,
}

async fn fixture() -> Result<Fixture, Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    write_catalog(&temp)?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let catalog = Arc::new(CatalogStore::load(CatalogFiles::from_data_dir(temp.path())).await?);
    let napcat = NapCatClient::new(NapCatConfig::new(
        Url::parse("http://127.0.0.1:9/")?,
        Duration::from_secs(1),
        None,
    )?)?;
    let identity = IdentityService::new(store.clone(), napcat.clone());
    let oauth_config = OAuthConfig::new(
        "client",
        None,
        None,
        Url::parse("http://127.0.0.1:9/oauth/authorize")?,
        Url::parse("http://127.0.0.1:9/oauth/token")?,
        vec!["read_player".to_owned()],
    )?;
    let scores = Arc::new(PlayerScoreService::with_lxns(
        store.clone(),
        Arc::clone(&catalog),
        DivingFishScoreClient::new(DivingFishClient::with_base_urls(
            "http://127.0.0.1:9/api/",
            "http://127.0.0.1:9/covers/",
        )?),
        OAuthService::new(store.clone(), LxnsOAuthClient::new(oauth_config)?),
        LxnsScoreEndpoint::new(
            Url::parse("http://127.0.0.1:9/api/v0/")?,
            Duration::from_secs(1),
        )?,
    ));
    let service = RankingService::new(store.clone(), napcat, identity, scores, catalog);
    Ok(Fixture {
        _temp: temp,
        store,
        service,
    })
}

async fn send<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    value: Value,
) -> Result<(), io::Error> {
    let mut bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await
}

async fn receive<R: tokio::io::AsyncRead + Unpin>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
) -> Result<Value, io::Error> {
    let line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::other("missing MCP response"))?;
    serde_json::from_str(&line).map_err(io::Error::other)
}

fn write_catalog(temp: &TempDir) -> Result<(), io::Error> {
    let files = [
        (
            "lxns_song_list.json",
            r#"{"songs":[{"id":383,"title":"Link","artist":"A","genre":"maimai","bpm":150,"version":25000,"difficulties":{"standard":[{"difficulty":3,"level":"13","level_value":13.0,"notes":{}}]}}],"genres":[],"versions":[{"title":"PRiSM","version":25000}]}"#,
        ),
        (
            "divingfish_song_list.json",
            r#"[{"id":"383","title":"Link","type":"SD","ds":[1,2,3,13.0],"level":["1","2","3","13"],"charts":[{},{},{},{}],"basic_info":{"bpm":150,"from":"PRiSM","is_new":true}}]"#,
        ),
        ("lxns_alias_list.json", r#"{"aliases":[]}"#),
        ("music_alias.json", r#"{"content":[]}"#),
        ("custom_aliases.json", "{}"),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#),
        ("zh_s2t.json", "{}"),
    ];
    for (name, contents) in files {
        fs::write(temp.path().join(name), contents)?;
    }
    Ok(())
}

fn member() -> Result<RankingMember, maimai_core::ValidationError> {
    Ok(RankingMember {
        ordinal: 0,
        qq: QqId::new("10001")?,
        nickname: Some("qq-name".to_owned()),
        card: Some("group-card".to_owned()),
        display_name: "group-card".to_owned(),
        waterfish_nickname: Some("player".to_owned()),
        waterfish_username: None,
    })
}

fn chart() -> Result<CachedChart, Box<dyn Error + Send + Sync>> {
    Ok(CachedChart {
        key: ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::DivingFish, 383),
            ChartGeneration::Standard,
            Difficulty::Master,
        )?,
        title: "Link".to_owned(),
        level: "13".to_owned(),
        constant: None,
        achievements: Some(AchievementRate::from_decimal_str("100.0000")?),
        dx_score: Some(1000),
        rating: Some(300),
        original_rating: None,
        grade: Some("sss".to_owned()),
        full_combo: None,
        full_sync: None,
        version: "PRiSM".to_owned(),
        is_current: false,
        fit_constant: None,
    })
}
