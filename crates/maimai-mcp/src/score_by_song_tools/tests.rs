use std::{error::Error, fs, io, sync::Arc, time::Duration};

use maimai_app::{
    identity::IdentityDirectory,
    oauth::OAuthService,
    score_by_song::ScoreBySongService,
    score_by_song::{
        MusicIdScores, PlayerLookupRequest, ScoreBySongResult, SelectedSong, SongCandidate,
        SongSelection,
    },
    score_service::PlayerScoreService,
    scores::{Lookup, PlayerScoreProfile, SelectionReason, SourceSelection},
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_core::{QqId, ScoreSource, SongIdNamespace, SourceSongId};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig, RawJsonPayload,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_storage::StateStore;
use rmcp::service::QuitReason;
use secrecy::SecretString;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::mpsc,
};
use url::Url;

use super::{
    MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON, ScoreBySongDispatcher, score_by_song_server,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn duplex_success_matches_golden_and_uses_one_full_records_request() -> TestResult {
    let fixture = fixture().await?;
    let (api, mut requests) = mock_server(200, records_body(), Duration::ZERO).await?;
    let dispatcher = dispatcher(&fixture, &api)?;
    let response = call(
        dispatcher,
        json!({
            "qq":"10001","songQuery":"六兆年"
        }),
    )
    .await?;
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["structuredContent"]["musicIds"],
        json!([288, 10288])
    );
    assert_eq!(
        response["result"]["structuredContent"]["counts"],
        json!({
            "requested":2,"success":2,"failure":0
        })
    );
    assert_eq!(
        response["result"]["content"][0]["text"].as_str(),
        Some(include_str!("golden/success.txt").trim_end())
    );
    let request = requests.recv().await.ok_or("provider request missing")?;
    assert!(request.starts_with("GET /api/maimaidxprober/dev/player/records?qq=10001"));
    assert!(requests.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn timeout_and_provider_error_are_tool_failures_without_secret_leak() -> TestResult {
    let slow = fixture().await?;
    let (api, _) = mock_server(200, records_body(), Duration::from_millis(1_200)).await?;
    let timed_out = call(
        dispatcher(&slow, &api)?,
        json!({
            "qq":"10001","songQuery":"六兆年","timeoutMs":1000
        }),
    )
    .await?;
    assert_eq!(timed_out["result"]["isError"], true);
    assert_eq!(
        timed_out["result"]["structuredContent"]["error"]["code"],
        "TIMEOUT"
    );

    let failed = fixture().await?;
    let secret = "provider-secret-sentinel";
    let (api, _) = mock_server(
        500,
        json!({"developerToken":secret,"message":"failed"}).to_string(),
        Duration::ZERO,
    )
    .await?;
    let response = call(
        dispatcher(&failed, &api)?,
        json!({
            "qq":"10001","songQuery":"六兆年"
        }),
    )
    .await?;
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "SCORE_QUERY_FAILED"
    );
    assert!(!response.to_string().contains(secret));
    Ok(())
}

#[test]
fn main_and_public_contracts_keep_the_same_single_tool() -> TestResult {
    let main: Value = serde_json::from_str(MAIN_CONTRACT_JSON)?;
    let public: Value = serde_json::from_str(PUBLIC_CONTRACT_JSON)?;
    assert_eq!(main["tools"][0]["name"], "query_maimai_score_by_song");
    assert_eq!(main["tools"], public["tools"]);
    assert_eq!(main["tools"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[test]
fn ten_music_ids_keep_raw_once_at_top_level() -> TestResult {
    let qq = QqId::new("10001")?;
    let sentinel = "raw-unique-sentinel";
    let result = ScoreBySongResult {
        requested_at: OffsetDateTime::UNIX_EPOCH,
        requested_player: PlayerLookupRequest::Qq(qq.clone()),
        lookup: Lookup::Qq(qq),
        identity: None,
        song_query: "ten ids".to_owned(),
        selected_song: SelectedSong {
            candidate: SongCandidate {
                id: SourceSongId::numeric(SongIdNamespace::Lxns, 1),
                title: "Ten IDs".to_owned(),
                artist: "Artist".to_owned(),
                source: "lxns",
                available_generations: Default::default(),
                aliases: Vec::new(),
            },
            music_ids: (1..=10).collect(),
        },
        selection: SongSelection {
            auto_selected: false,
            selected_rank: 1,
            total_matches: 1,
            truncated: false,
            candidates: Vec::new(),
        },
        source: ScoreSource::DivingFish,
        source_selection: SourceSelection {
            preferred_source: ScoreSource::DivingFish,
            source: ScoreSource::DivingFish,
            reason: SelectionReason::Default,
        },
        player: PlayerScoreProfile::default(),
        scores: (1..=10)
            .map(|music_id| MusicIdScores {
                music_id,
                records: Vec::new(),
            })
            .collect(),
        raw: Some(RawJsonPayload::from_value(&json!({
            "sentinel": sentinel,
            "blob": "x".repeat(4_096),
        }))?),
    };
    let serialized = super::serialize::result(result)?;
    assert_eq!(serialized["scores"].as_array().map(Vec::len), Some(10));
    assert!(
        serialized["scores"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|item| item.get("raw").is_none())
    );
    let rendered = serialized.to_string();
    assert_eq!(rendered.matches(sentinel).count(), 1);
    assert!(rendered.len() < 20_000);
    Ok(())
}

struct Fixture {
    _temp: TempDir,
    store: StateStore,
    catalog: Arc<CatalogStore>,
}

async fn fixture() -> Result<Fixture, Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    write_catalog(&temp)?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    store
        .set_diving_fish_developer_token(
            &SecretString::from("developer-secret"),
            OffsetDateTime::UNIX_EPOCH,
        )
        .await?;
    let catalog = Arc::new(CatalogStore::load(CatalogFiles::from_data_dir(temp.path())).await?);
    Ok(Fixture {
        _temp: temp,
        store,
        catalog,
    })
}

fn dispatcher(
    fixture: &Fixture,
    api: &Url,
) -> Result<ScoreBySongDispatcher, Box<dyn Error + Send + Sync>> {
    let identities = IdentityDirectory::new(fixture.store.clone());
    let oauth_config = OAuthConfig::new(
        "client",
        None,
        None,
        Url::parse("http://127.0.0.1:9/oauth/authorize")?,
        Url::parse("http://127.0.0.1:9/oauth/token")?,
        vec!["read_player".to_owned()],
    )?;
    let api_base = api.join("api/")?;
    let cover_base = api.join("covers/")?;
    let scores = Arc::new(PlayerScoreService::with_lxns(
        fixture.store.clone(),
        Arc::clone(&fixture.catalog),
        DivingFishScoreClient::new(DivingFishClient::with_base_urls(
            api_base.as_str(),
            cover_base.as_str(),
        )?),
        OAuthService::new(fixture.store.clone(), LxnsOAuthClient::new(oauth_config)?),
        LxnsScoreEndpoint::new(
            Url::parse("http://127.0.0.1:9/api/v0/")?,
            Duration::from_secs(1),
        )?,
    ));
    Ok(ScoreBySongDispatcher::new(ScoreBySongService::new(
        identities,
        Arc::clone(&fixture.catalog),
        scores,
    )))
}

async fn call(dispatcher: ScoreBySongDispatcher, arguments: Value) -> Result<Value, io::Error> {
    let server = score_by_song_server(MAIN_CONTRACT_JSON, dispatcher)
        .map_err(|error| io::Error::other(error.to_string()))?;
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
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"score-by-song-test","version":"1"}
        }}),
    )
    .await?;
    let _ = receive(&mut lines).await?;
    send(
        &mut write,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await?;
    send(
        &mut write,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"query_maimai_score_by_song","arguments":arguments
        }}),
    )
    .await?;
    let response = receive(&mut lines).await?;
    write.shutdown().await?;
    drop(write);
    task.await.map_err(io::Error::other)??;
    Ok(response)
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

async fn mock_server(
    status: u16,
    body: String,
    delay: Duration,
) -> Result<(Url, mpsc::Receiver<String>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(2);
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let Ok(request) = read_request(&mut stream).await else {
            return;
        };
        let _ = sender.send(request).await;
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });
    Ok((Url::parse(&format!("http://{address}/"))?, receiver))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<String, io::Error> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

fn records_body() -> String {
    json!({
        "nickname":"Tester","rating":15000,
        "records":[
            {"id":288,"title":"六兆年と一夜物語","type":"SD","level":"13",
                "level_index":3,"ds":"13.0","achievements":"99.9000","ra":280},
            {"id":10288,"title":"六兆年と一夜物語","type":"DX","level":"13+",
                "level_index":3,"ds":"13.8","achievements":"100.0000","ra":300}
        ]
    })
    .to_string()
}

fn write_catalog(temp: &TempDir) -> Result<(), io::Error> {
    let files = [
        (
            "lxns_song_list.json",
            json!({"songs":[{"id":288,"title":"六兆年と一夜物語","artist":"kemu","genre":"niconico","bpm":186,"version":25000,"difficulties":{"standard":[{"difficulty":3,"level":"13","level_value":13.0,"notes":{}}],"dx":[{"difficulty":3,"level":"13+","level_value":13.8,"notes":{}}]}}],"genres":[],"versions":[{"title":"PRiSM","version":25000}]}).to_string(),
        ),
        (
            "divingfish_song_list.json",
            json!([
                {"id":"288","title":"六兆年と一夜物語","type":"SD","ds":[1,2,3,13.0],"level":["1","2","3","13"],"charts":[{},{},{},{}],"basic_info":{"artist":"kemu","genre":"niconico","bpm":186,"from":"PRiSM","is_new":false}},
                {"id":"10288","title":"六兆年と一夜物語","type":"DX","ds":[1,2,3,13.8],"level":["1","2","3","13+"],"charts":[{},{},{},{}],"basic_info":{"artist":"kemu","genre":"niconico","bpm":186,"from":"PRiSM","is_new":false}}
            ]).to_string(),
        ),
        ("lxns_alias_list.json", r#"{"aliases":[]}"#.to_owned()),
        ("music_alias.json", r#"{"content":[]}"#.to_owned()),
        ("custom_aliases.json", r#"{"六兆年":["288"]}"#.to_owned()),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#.to_owned()),
        ("zh_s2t.json", "{}".to_owned()),
    ];
    for (name, contents) in files {
        fs::write(temp.path().join(name), contents)?;
    }
    Ok(())
}
