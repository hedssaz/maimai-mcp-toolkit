use std::{error::Error, fs, io, sync::Arc};

use maimai_app::catalog_refresh::{CatalogRefreshService, EnabledSources, job::CatalogRefreshJobs};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_providers::{CatalogSourceClient, CatalogSourceConfig};
use maimai_storage::{CatalogRefreshJobStart, CatalogRefreshJobStore};
use rmcp::service::QuitReason;
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

use super::{CatalogDispatcher, TOOLS, execute};

const MAIN_CONTRACT_JSON: &str = include_str!("../../../../contracts/main/catalog.json");
const PUBLIC_CONTRACT_JSON: &str = include_str!("../../../../contracts/public/catalog.json");

#[test]
fn frozen_catalog_surface_exposes_all_fourteen_tools() -> Result<(), Box<dyn Error>> {
    let public_tools = TOOLS
        .iter()
        .copied()
        .filter(|tool| *tool != "query_chart_history")
        .collect::<Vec<_>>();
    for (contract_json, expected) in [
        (MAIN_CONTRACT_JSON, TOOLS.as_slice()),
        (PUBLIC_CONTRACT_JSON, public_tools.as_slice()),
    ] {
        let contract = crate::contract::SurfaceContract::parse(contract_json)?.retain_tools(&TOOLS);
        assert_eq!(
            contract
                .tools()
                .iter()
                .map(|tool| tool.name())
                .collect::<Vec<_>>(),
            expected
        );
    }
    let main = crate::contract::SurfaceContract::parse(MAIN_CONTRACT_JSON)?;
    let public = crate::contract::SurfaceContract::parse(PUBLIC_CONTRACT_JSON)?;
    let song_id_description = |contract: &crate::contract::SurfaceContract| {
        contract
            .tools()
            .iter()
            .find(|tool| tool.name() == "add_maimai_alias")
            .and_then(|tool| tool.input_schema().get("properties"))
            .and_then(|properties| properties.get("song_id"))
            .and_then(|song_id| song_id.get("description"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    assert_ne!(song_id_description(&main), song_id_description(&public));
    Ok(())
}

#[test]
fn typed_filters_cover_fit_region_tag_lock_and_release() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let snapshot = fixture.files.load()?;
    let (value, _) = execute(
        &snapshot,
        "search_maimai_songs",
        object(json!({
            "query": "Alpha",
            "song_type": "dx",
            "difficulty": "Master",
            "fit_diff": "13.1-13.3",
            "fit_label": "虚高",
            "region_has": ["jp", "intl"],
            "is_new": true,
            "is_new_source": "jp",
            "is_locked": true,
            "tag": "Jacks",
            "released_after": "2024",
            "released_before": "2024-12",
            "format": "json"
        }))?,
    )?;
    assert_eq!(value["count"], 1);
    assert_eq!(value["songs"][0]["matched_charts"][0]["fit_label"], "虚高");
    assert_eq!(value["songs"][0]["matched_charts"][0]["tags"][0]["id"], 1);
    assert_eq!(
        value["songs"][0]["matched_charts"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(value["songs"][0]["matched_charts"][0]["source"], "jp");
    Ok(())
}

#[tokio::test]
async fn public_refresh_rejects_disabled_sources_and_accepts_legacy_parameter_aliases()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = Arc::new(CatalogStore::load(fixture.files.clone()).await?);
    let service = Arc::new(CatalogRefreshService::new(
        Arc::new(CatalogSourceClient::new(CatalogSourceConfig::default())?),
        Arc::clone(&store),
        EnabledSources::public(),
    )?);
    let disabled = super::execute_call(
        &store,
        Some(&service),
        None,
        "refresh_maimai_sources",
        object(json!({"source":"dxdata","check_only":true}))?,
    )
    .await
    .err()
    .ok_or("expected disabled public source")?;
    assert!(disabled.to_string().starts_with("INVALID_SOURCE:"));

    let (value, _) = super::execute_call(
        &store,
        Some(&service),
        None,
        "refresh_maimai_sources",
        object(json!({
            "source":"plate_data",
            "source_ttl_days":"0.0208",
            "check_only":"yes",
            "timeout_seconds":"1"
        }))?,
    )
    .await?;
    assert_eq!(value["requested_sources"], json!(["plate"]));
    assert_eq!(value["check_only"], true);
    assert_eq!(value["due_sources"], json!(["plate"]));

    for arguments in [
        json!({"source":"plate","timeout_seconds":0}),
        json!({"source":"plate","background":true}),
    ] {
        assert!(
            super::execute_call(
                &store,
                Some(&service),
                None,
                "refresh_maimai_sources",
                object(arguments)?,
            )
            .await
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn real_catalog_keeps_source_provenance_and_hidden_search_keys() -> Result<(), Box<dyn Error>> {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let snapshot = CatalogFiles::from_data_dir(data_dir).load()?;
    let (selector, _) = execute(
        &snapshot,
        "search_maimai_songs",
        object(json!({"query":"id10574","limit":5,"format":"json"}))?,
    )?;
    assert_eq!(selector["total_matches"], 1);
    let song = &selector["songs"][0];
    assert_eq!(song["title"], "Selector");
    assert!(
        song["source_id_aliases"]["divingfish"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == "10574"))
    );
    assert!(
        song["matched_charts"]
            .as_array()
            .is_some_and(|charts| charts.iter().any(|chart| chart["fit_source_id"] == "10574"))
    );
    assert!(song["source_fields"]["cn"]["version"].is_number());

    let (dx_only, _) = execute(
        &snapshot,
        "search_maimai_songs",
        object(json!({"query":"id1853","limit":5,"format":"json"}))?,
    )?;
    assert_eq!(dx_only["songs"][0]["title"], "Help me, ERINNNNNN!!");
    assert_eq!(dx_only["songs"][0]["id"], "1853");
    assert_eq!(dx_only["songs"][0]["source_ids"]["divingfish"], "11853");

    let (pinyin, _) = execute(
        &snapshot,
        "search_maimai_songs",
        object(json!({"query":"liuzhaonian","limit":5,"format":"json"}))?,
    )?;
    assert_eq!(pinyin["songs"][0]["title"], "六兆年と一夜物語");
    assert_eq!(pinyin["songs"][0]["match"]["field"], "pinyin");
    assert!(
        pinyin["songs"][0]["aliases"]
            .as_array()
            .is_some_and(|aliases| aliases.iter().all(|value| value != "liuzhaonian"))
    );

    let (keyword, _) = execute(
        &snapshot,
        "search_maimai_songs",
        object(json!({"query":"Demon Slayer","limit":5,"format":"json"}))?,
    )?;
    assert_eq!(keyword["songs"][0]["title"], "紅蓮華");
    assert_eq!(keyword["songs"][0]["match"]["field"], "keyword");

    let (cn_year, _) = execute(
        &snapshot,
        "search_maimai_songs",
        object(json!({
            "version":"2025",
            "region_has":"cn",
            "genre":"maimai",
            "limit":200,
            "format":"json"
        }))?,
    )?;
    let songs = cn_year["songs"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("songs must be an array"))?;
    assert!(!songs.is_empty());
    for song in songs {
        assert!(
            song["source_fields"]["cn"]["version"]
                .as_u64()
                .is_some_and(|version| version.to_string().starts_with("25"))
        );
        assert!(
            song["matched_charts"]
                .as_array()
                .is_some_and(|charts| charts.iter().all(|chart| chart["source"] == "cn"))
        );
    }
    Ok(())
}

#[tokio::test]
async fn catalog_dispatcher_completes_a_real_duplex_mcp_call() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = Arc::new(CatalogStore::load(fixture.files.clone()).await?);
    let refresh = Arc::new(CatalogRefreshService::new(
        Arc::new(CatalogSourceClient::new(CatalogSourceConfig::default())?),
        Arc::clone(&store),
        EnabledSources::main(),
    )?);
    let job_store =
        CatalogRefreshJobStore::open(fixture._root.path().join("refresh-state.db")).await?;
    let jobs = Arc::new(CatalogRefreshJobs::open(Arc::clone(&refresh), job_store.clone()).await?);
    let dispatcher = CatalogDispatcher::new(store, refresh, Arc::clone(&jobs));
    let contract =
        crate::contract::SurfaceContract::parse(MAIN_CONTRACT_JSON)?.retain_tools(&TOOLS);
    let server = crate::ContractServer::new(contract, dispatcher);
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
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
    let (read, mut write) = tokio::io::split(client_io);
    let mut lines = BufReader::new(read).lines();
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "protocolVersion":"2024-11-05",
                "capabilities":{},
                "clientInfo":{"name":"catalog-test","version":"1"}
            }
        }),
    )
    .await?;
    assert_eq!(
        read_message(&mut lines).await?["result"]["protocolVersion"],
        "2024-11-05"
    );
    write_message(
        &mut write,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await?;
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":6,
            "method":"tools/call",
            "params":{
                "name":"refresh_maimai_sources",
                "arguments":{"sources":"plate","ttl_days":0.0208,"check_only":true}
            }
        }),
    )
    .await?;
    let refreshed = read_message(&mut lines).await?;
    assert_eq!(refreshed["result"]["isError"], false);
    assert!(refreshed["result"].get("structuredContent").is_none());
    assert_eq!(
        refreshed["result"]["content"][0]["text"],
        "源刷新: force=False check_only=True ttl_days=0.0208\n应刷新: plate\n已刷新: -\n已跳过: -\n失败: -\n- plate: expired=True age_days=- mtime=-"
    );
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":7,
            "method":"tools/call",
            "params":{
                "name":"refresh_maimai_sources",
                "arguments":{
                    "sources":"lxns",
                    "ttl_days":999999,
                    "background":true,
                    "format":"json"
                }
            }
        }),
    )
    .await?;
    let started = read_message(&mut lines).await?;
    assert_eq!(started["result"]["isError"], false);
    assert!(started["result"].get("structuredContent").is_none());
    let started_text = started["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| io::Error::other("background start text missing"))?;
    let started_value: Value = serde_json::from_str(started_text)?;
    let job_id = started_value["jobId"]
        .as_str()
        .ok_or_else(|| io::Error::other("background job id missing"))?
        .to_owned();
    assert_eq!(started_value["dueSources"], 0);
    assert_eq!(started_value["sourceStates"]["lxns"]["expired"], false);
    assert!(started_value["sourceStates"]["lxns"]["age_days"].is_number());
    let mut terminal = Value::Null;
    for attempt in 0..50 {
        write_message(
            &mut write,
            &json!({
                "jsonrpc":"2.0",
                "id":100 + attempt,
                "method":"tools/call",
                "params":{
                    "name":"refresh_maimai_sources_job_status",
                    "arguments":{"jobId":&job_id,"format":"json"}
                }
            }),
        )
        .await?;
        let status = read_message(&mut lines).await?;
        let text = status["result"]["content"][0]["text"]
            .as_str()
            .ok_or_else(|| io::Error::other("job status text missing"))?;
        terminal = serde_json::from_str(text)?;
        if terminal["status"] == "finished" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(terminal["status"], "finished");
    assert_eq!(terminal["completedSources"], 1);
    assert_eq!(terminal["succeededSources"], json!(["lxns"]));
    let encoded = terminal.to_string();
    assert!(
        !encoded.contains("python")
            && !encoded.contains(fixture._root.path().to_string_lossy().as_ref())
    );
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":8,
            "method":"tools/call",
            "params":{
                "name":"refresh_maimai_sources_job_status",
                "arguments":{"jobId":&job_id}
            }
        }),
    )
    .await?;
    let status_text = read_message(&mut lines).await?;
    assert_eq!(
        status_text["result"]["content"][0]["text"],
        "刷新进度: status=finished completed=1/1\n成功: lxns\n失败: -\nmessage: 刷新完成: 成功 1/1\n  ✅ lxns: rc=0 dur=0s"
    );
    let active = job_store
        .start_catalog_refresh_job(
            &["lxns".to_owned()],
            &["lxns".to_owned()],
            OffsetDateTime::now_utc(),
        )
        .await?;
    let active_id = match active {
        CatalogRefreshJobStart::Started(job) => job.id,
        CatalogRefreshJobStart::AlreadyRunning(_) => {
            return Err(io::Error::other("force fixture found an active job").into());
        }
    };
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":9,
            "method":"tools/call",
            "params":{
                "name":"refresh_maimai_sources",
                "arguments":{"sources":"lxns","force":true,"format":"json"}
            }
        }),
    )
    .await?;
    let forced = read_message(&mut lines).await?;
    assert_eq!(forced["result"]["isError"], false);
    let forced_text = forced["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| io::Error::other("force-only job start text missing"))?;
    let forced_value: Value = serde_json::from_str(forced_text)?;
    assert_eq!(forced_value["background"], true);
    assert_eq!(forced_value["started"], false);
    assert_eq!(forced_value["jobId"], active_id.to_string());
    assert_eq!(
        job_store
            .interrupt_catalog_refresh_jobs(OffsetDateTime::now_utc())
            .await?,
        1
    );
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/call",
            "params":{
                "name":"search_maimai_songs",
                "arguments":{"query":"Alpha","format":"json"}
            }
        }),
    )
    .await?;
    let response = read_message(&mut lines).await?;
    assert_eq!(response["result"]["isError"], false);
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| io::Error::other("tool text missing"))?;
    let value: Value = serde_json::from_str(text)?;
    assert_eq!(value["songs"][0]["title"], "Alpha");
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":10,
            "method":"tools/call",
            "params":{
                "name":"find_score_combinations",
                "arguments":{
                    "query":"Alpha",
                    "song_type":"dx",
                    "difficulty":"Master",
                    "score_mode":"dxscore",
                    "target_score":0,
                    "allowed_judgments":{"tap":"miss"},
                    "max_solutions":0,
                    "format":"json"
                }
            }
        }),
    )
    .await?;
    let scored = read_message(&mut lines).await?;
    assert_eq!(scored["result"]["isError"], false);
    let scored: Value = serde_json::from_str(
        scored["result"]["content"][0]["text"]
            .as_str()
            .ok_or_else(|| io::Error::other("scoring text missing"))?,
    )?;
    assert_eq!(scored["calculated"], true);
    assert_eq!(scored["lookup"]["song"]["title"], "Alpha");
    assert_eq!(scored["matching_combination_count"], 1);
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":5,
            "method":"tools/call",
            "params":{
                "name":"delete_maimai_alias",
                "arguments":{"song_id":1,"alias":"missing"}
            }
        }),
    )
    .await?;
    let missing = read_message(&mut lines).await?;
    assert_eq!(missing["result"]["isError"], true);
    assert!(missing["result"].get("structuredContent").is_none());
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"tools/call",
            "params":{
                "name":"add_maimai_alias",
                "arguments":{"song_id":1,"alias":"duplex-alias"}
            }
        }),
    )
    .await?;
    let added = read_message(&mut lines).await?;
    assert_eq!(added["result"]["isError"], false);
    assert!(added["result"].get("structuredContent").is_none());
    assert!(
        added["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.starts_with("别名已新增: duplex-alias -> Alpha"))
    );
    write_message(
        &mut write,
        &json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"tools/call",
            "params":{
                "name":"search_maimai_songs",
                "arguments":{"query":"duplex-alias","format":"json"}
            }
        }),
    )
    .await?;
    let searched = read_message(&mut lines).await?;
    let text = searched["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| io::Error::other("search text missing"))?;
    let value: Value = serde_json::from_str(text)?;
    assert_eq!(value["songs"][0]["title"], "Alpha");
    write.shutdown().await?;
    drop(write);
    server_task.await??;
    jobs.shutdown().await?;
    Ok(())
}

async fn write_message<W>(writer: &mut W, value: &Value) -> Result<(), io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut encoded = serde_json::to_vec(value).map_err(io::Error::other)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await
}

async fn read_message<R>(lines: &mut Lines<BufReader<R>>) -> Result<Value, io::Error>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "MCP response missing"))?;
    serde_json::from_str(&line).map_err(io::Error::other)
}

pub(super) fn object(value: Value) -> Result<Map<String, Value>, Box<dyn Error>> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| std::io::Error::other("fixture must be object").into())
}

pub(super) struct Fixture {
    _root: TempDir,
    pub(super) files: CatalogFiles,
}

impl Fixture {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let root = TempDir::new()?;
        let files = CatalogFiles::from_data_dir(fs::canonicalize(root.path())?);
        fs::write(
            &files.lxns_song_list,
            r#"{"songs":[{"id":1,"title":"Alpha","artist":"Alice","genre":"game","bpm":160,"version":10000,"difficulties":{"dx":[{"difficulty":3,"level":"13+","level_value":13.5,"note_designer":"Carol","notes":{"tap":1}}]}}],"genres":[{"title":"Game","genre":"game"}],"versions":[{"title":"PRiSM","version":10000}]}"#,
        )?;
        fs::write(
            &files.diving_fish_song_list,
            r#"[{"id":"10001","title":"Alpha","type":"DX","ds":[0,0,0,13.5],"level":["1","1","1","13+"],"charts":[{"notes":[],"charter":"-"},{"notes":[],"charter":"-"},{"notes":[],"charter":"-"},{"notes":[1,0,0,0,0],"charter":"Carol"}],"basic_info":{"artist":"Alice","genre":"Game","bpm":160,"from":"PRiSM"}}]"#,
        )?;
        fs::write(&files.lxns_alias_list, r#"{"aliases":[]}"#)?;
        fs::write(&files.yuzu_alias_list, r#"{"content":[]}"#)?;
        fs::write(&files.custom_aliases, "{}")?;
        fs::write(&files.pinyin_aliases, r#"{"aliases":[]}"#)?;
        fs::write(
            &files.simplified_to_traditional,
            r#"{"测":"測","试":"試","别":"別"}"#,
        )?;
        if let Some(path) = &files.traditional_to_simplified {
            fs::write(path, r#"{"測":"测","試":"试","別":"别"}"#)?;
        }
        if let Some(path) = &files.artist_aliases {
            fs::write(path, "{}")?;
        }
        if let Some(path) = &files.charter_aliases {
            fs::write(path, "{}")?;
        }
        let dxdata = files
            .dxdata
            .as_ref()
            .ok_or_else(|| std::io::Error::other("dxdata path missing"))?;
        fs::write(
            dxdata,
            r#"{"songs":[{"songId":"Alpha","title":"Alpha","artist":"Alice","category":"Game","bpm":160,"isNew":true,"isLocked":true,"sheets":[{"type":"dx","difficulty":"master","level":"13+","internalLevelValue":13.5,"noteDesigner":"Carol","noteCounts":{"tap":1,"hold":0,"slide":0,"touch":0,"break":0},"regions":{"jp":true,"intl":true,"usa":false,"cn":false},"version":"PRiSM","internalId":10001,"releaseDate":"2024-06-30","multiverInternalLevelValue":{"BUDDiES":13.4}}]}],"versions":[{"version":"BUDDiES"},{"version":"PRiSM"}]}"#,
        )?;
        let stats = files
            .chart_stats
            .as_ref()
            .ok_or_else(|| std::io::Error::other("stats path missing"))?;
        fs::write(
            stats,
            r#"{"charts":{"10001":[{"fit_diff":0},{"fit_diff":0},{"fit_diff":0},{"fit_diff":13.2}]}}"#,
        )?;
        let tags = files
            .tags
            .as_ref()
            .ok_or_else(|| std::io::Error::other("tags path missing"))?;
        fs::write(
            tags,
            r#"{"tags":[{"id":1,"localized_name":{"en":"Jacks","zh-Hans":"纵连"}}],"tagSongs":[{"song_id":"Alpha","sheet_type":"dx","sheet_difficulty":"master","tag_id":1}]}"#,
        )?;
        Ok(Self { _root: root, files })
    }
}
