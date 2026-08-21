use std::{error::Error, io, time::Duration};

use maimai_app::identity::{IdentityService, ResetHour};
use maimai_core::{GroupId, QqId};
use maimai_providers::{NapCatClient, NapCatConfig};
use maimai_storage::{IdentityGroupSnapshot, IdentitySnapshot, IdentitySnapshotMember, StateStore};
use rmcp::{serve_server, service::QuitReason};
use serde_json::{Value, json};
use tempfile::TempDir;
use time::{Date, Month, PrimitiveDateTime, Time};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines, ReadHalf, WriteHalf},
    net::TcpListener,
    task::JoinHandle,
    time::sleep,
};
use url::Url;

use super::{DisplayOffset, IdentityDispatcher, MAIN_CONTRACT_JSON, identity_server};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;
type TestLines = Lines<BufReader<ReadHalf<tokio::io::DuplexStream>>>;

struct MockResponse {
    status: u16,
    body: String,
    delay: Duration,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplex_surface_preserves_all_five_tools_text_and_shapes() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("identity.db");
    let store = StateStore::open(&database).await?;
    store
        .replace_identity_snapshot(&IdentitySnapshot {
            fetched_at: timestamp(2026, Month::August, 18, 2, 0)?,
            friends: vec![member("10001", "Alice", None)?],
            groups: vec![IdentityGroupSnapshot {
                group_id: GroupId::new("20001")?,
                group_name: Some("Mai Group".to_owned()),
                member_count: Some(2),
                members: vec![
                    member("10001", "Alice", Some("Same|Name\nA"))?,
                    member("10002", "Bob", Some("Same|Name\nA"))?,
                ],
            }],
        })
        .await?;
    let (base_url, _) = mock_server(success_responses(Duration::from_millis(80))).await?;
    let service = IdentityService::new(store.clone(), client(base_url.clone())?);
    let dispatcher = IdentityDispatcher::with_display_offset(
        service,
        ResetHour::new(14)?,
        DisplayOffset::from_hours(9)?,
    );
    let (mut writer, mut lines, server_task) = start_server(dispatcher).await?;

    initialize(&mut writer, &mut lines).await?;
    write_message(
        &mut writer,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await?;
    let listed = read_message(&mut lines).await?;
    assert_eq!(listed["result"]["tools"].as_array().map(Vec::len), Some(5));

    let cache = call_tool(
        &mut writer,
        &mut lines,
        3,
        "qq_identity_cache_status",
        json!({}),
    )
    .await?;
    assert_eq!(cache["result"]["isError"], false);
    assert!(text(&cache)?.contains("2026-08-18 11:00:00 +09:00"));
    assert_eq!(
        cache["result"]["structuredContent"]["stats"]["uniqueUsers"],
        2
    );

    let job = call_tool(
        &mut writer,
        &mut lines,
        4,
        "qq_identity_job_status",
        json!({}),
    )
    .await?;
    assert_eq!(job["result"]["structuredContent"]["job"], Value::Null);
    assert_eq!(text(&job)?, "当前没有 QQ 身份缓存刷新任务。");

    let resolved = call_tool(
        &mut writer,
        &mut lines,
        5,
        "resolve_qq_identity",
        json!({"query":"Same|Name", "groupId":"20001", "maxResults":10}),
    )
    .await?;
    assert_eq!(resolved["result"]["structuredContent"]["ambiguous"], true);
    assert_eq!(
        resolved["result"]["structuredContent"]["matches"][0]["matchScore"],
        60
    );
    assert!(text(&resolved)?.contains("Same\\|Name A"));

    let identity = call_tool(
        &mut writer,
        &mut lines,
        6,
        "get_qq_identity",
        json!({"qq":"10001", "groupId":"20001"}),
    )
    .await?;
    assert_eq!(
        identity["result"]["structuredContent"]["identity"]["preferredGroup"]["groupNickname"],
        "Same|Name\nA"
    );
    assert!(text(&identity)?.contains("当前群昵称: Same|Name\nA（Mai Group）"));

    let started = call_tool(
        &mut writer,
        &mut lines,
        7,
        "refresh_qq_identity_cache",
        json!({
            "forceRefresh": true,
            "napcatBaseUrl": base_url.as_str(),
            "timeoutMs": 2000,
            "groupDelayMs": 0
        }),
    )
    .await?;
    assert_eq!(started["result"]["structuredContent"]["started"], true);
    assert_eq!(
        started["result"]["structuredContent"]["job"]["status"],
        "running"
    );
    assert!(text(&started)?.starts_with("QQ 身份缓存刷新已启动。"));

    let coalesced = call_tool(
        &mut writer,
        &mut lines,
        8,
        "refresh_qq_identity_cache",
        json!({
            "forceRefresh": true,
            "napcatBaseUrl": base_url.as_str(),
            "timeoutMs": 2000,
            "groupDelayMs": 0
        }),
    )
    .await?;
    assert_eq!(coalesced["result"]["structuredContent"]["started"], false);
    assert!(text(&coalesced)?.contains("仍在进行，未启动新任务"));

    let completed = wait_for_job(&mut writer, &mut lines, 20, "completed").await?;
    assert_eq!(
        completed["result"]["structuredContent"]["job"]["stats"]["uniqueUsers"],
        2
    );
    let fresh = call_tool(
        &mut writer,
        &mut lines,
        30,
        "refresh_qq_identity_cache",
        json!({"groupDelayMs":0}),
    )
    .await?;
    assert_eq!(fresh["result"]["structuredContent"]["started"], false);
    assert!(text(&fresh)?.contains("当前缓存仍在 1 天有效期内"));

    let invalid = call_tool(
        &mut writer,
        &mut lines,
        31,
        "refresh_qq_identity_cache",
        json!({"timeoutMs":999}),
    )
    .await?;
    assert_eq!(invalid["result"]["isError"], true);
    assert_eq!(
        invalid["result"]["structuredContent"]["error"],
        json!({
            "code":"INVALID_INPUT",
            "message":"timeoutMs 必须是 1000 到 60000 之间的整数。",
            "status":null,
            "body":null
        })
    );
    let invalid_base = call_tool(
        &mut writer,
        &mut lines,
        32,
        "refresh_qq_identity_cache",
        json!({"napcatBaseUrl":"http://127.0.0.1:9/"}),
    )
    .await?;
    assert_eq!(invalid_base["result"]["isError"], true);
    assert_eq!(
        invalid_base["result"]["structuredContent"]["error"]["code"],
        "INVALID_INPUT"
    );
    assert!(text(&invalid_base)?.contains("只允许使用进程启动时配置的地址"));

    shutdown(writer, server_task).await?;
    store.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplex_job_error_body_is_structured_and_redacted() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("identity.db");
    let store = StateStore::open(&database).await?;
    let responses = vec![
        ok(json!([])),
        ok(json!([{"group_id":20001,"group_name":"G"}])),
        MockResponse {
            status: 500,
            body: json!({"authorization":"SECRET_SENTINEL", "detail":"safe"}).to_string(),
            delay: Duration::ZERO,
        },
    ];
    let (base_url, _) = mock_server(responses).await?;
    let service = IdentityService::new(store.clone(), client(base_url.clone())?);
    let dispatcher = IdentityDispatcher::new(service, ResetHour::new(14)?)?;
    let (mut writer, mut lines, server_task) = start_server(dispatcher).await?;
    initialize(&mut writer, &mut lines).await?;
    let _ = call_tool(
        &mut writer,
        &mut lines,
        2,
        "refresh_qq_identity_cache",
        json!({
            "forceRefresh":true,
            "napcatBaseUrl":base_url.as_str(),
            "timeoutMs":2000,
            "groupDelayMs":0
        }),
    )
    .await?;
    let failed = wait_for_job(&mut writer, &mut lines, 10, "failed").await?;
    let encoded = serde_json::to_string(&failed)?;
    assert!(!encoded.contains("SECRET_SENTINEL"));
    assert!(encoded.contains("[REDACTED]"));
    assert_eq!(
        failed["result"]["structuredContent"]["job"]["error"]["status"],
        500
    );
    shutdown(writer, server_task).await?;
    store.close().await;
    Ok(())
}

async fn start_server(
    dispatcher: IdentityDispatcher,
) -> Result<
    (
        WriteHalf<tokio::io::DuplexStream>,
        TestLines,
        JoinHandle<Result<(), io::Error>>,
    ),
    Box<dyn Error + Send + Sync>,
> {
    let server = identity_server(MAIN_CONTRACT_JSON, dispatcher)?;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        let running = serve_server(server, server_io)
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
    let (read, write) = tokio::io::split(client_io);
    Ok((write, BufReader::new(read).lines(), task))
}

async fn initialize(
    writer: &mut WriteHalf<tokio::io::DuplexStream>,
    lines: &mut TestLines,
) -> Result<(), io::Error> {
    write_message(
        writer,
        &json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{
                "protocolVersion":"2024-11-05","capabilities":{},
                "clientInfo":{"name":"identity-test","version":"1"}
            }
        }),
    )
    .await?;
    let initialized = read_message(lines).await?;
    if initialized["result"]["serverInfo"]["name"] != "qq-identity-mcp" {
        return Err(io::Error::other("identity MCP initialization failed"));
    }
    write_message(
        writer,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await
}

async fn call_tool(
    writer: &mut WriteHalf<tokio::io::DuplexStream>,
    lines: &mut TestLines,
    id: u64,
    name: &str,
    arguments: Value,
) -> Result<Value, io::Error> {
    write_message(
        writer,
        &json!({
            "jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
    )
    .await?;
    read_message(lines).await
}

async fn wait_for_job(
    writer: &mut WriteHalf<tokio::io::DuplexStream>,
    lines: &mut TestLines,
    first_id: u64,
    expected: &str,
) -> Result<Value, io::Error> {
    for offset in 0..100 {
        let response = call_tool(
            writer,
            lines,
            first_id + offset,
            "qq_identity_job_status",
            json!({}),
        )
        .await?;
        if response["result"]["structuredContent"]["job"]["status"] == expected {
            return Ok(response);
        }
        sleep(Duration::from_millis(10)).await;
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "identity job did not reach expected status",
    ))
}

async fn shutdown(
    mut writer: WriteHalf<tokio::io::DuplexStream>,
    task: JoinHandle<Result<(), io::Error>>,
) -> TestResult {
    writer.shutdown().await?;
    drop(writer);
    task.await??;
    Ok(())
}

async fn write_message(
    writer: &mut WriteHalf<tokio::io::DuplexStream>,
    message: &Value,
) -> Result<(), io::Error> {
    let mut encoded = serde_json::to_vec(message).map_err(io::Error::other)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await
}

async fn read_message(lines: &mut TestLines) -> Result<Value, io::Error> {
    let line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "MCP response missing"))?;
    serde_json::from_str(&line).map_err(io::Error::other)
}

fn text(response: &Value) -> Result<&str, io::Error> {
    response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| io::Error::other("MCP text content missing"))
}

fn success_responses(delay: Duration) -> Vec<MockResponse> {
    vec![
        MockResponse {
            status: 200,
            body: json!({
                "status":"ok","retcode":0,
                "data":[{"user_id":10001,"nickname":"Alice","remark":"ignore"}]
            })
            .to_string(),
            delay,
        },
        ok(json!([{
            "group_id":20001,"group_name":"Mai Group","member_count":2
        }])),
        ok(json!([
            {"user_id":10001,"nickname":"Alice","card":"Captain"},
            {"user_id":10002,"nickname":"Bob","card":""}
        ])),
    ]
}

fn member(
    qq: &str,
    nickname: &str,
    card: Option<&str>,
) -> Result<IdentitySnapshotMember, maimai_core::ValidationError> {
    Ok(IdentitySnapshotMember {
        qq: QqId::new(qq)?,
        nickname: Some(nickname.to_owned()),
        card: card.map(str::to_owned),
    })
}

fn timestamp(
    year: i32,
    month: Month,
    day: u8,
    hour: u8,
    minute: u8,
) -> Result<time::OffsetDateTime, time::error::ComponentRange> {
    Ok(PrimitiveDateTime::new(
        Date::from_calendar_date(year, month, day)?,
        Time::from_hms(hour, minute, 0)?,
    )
    .assume_utc())
}

fn client(base_url: Url) -> Result<NapCatClient, maimai_providers::NapCatError> {
    NapCatClient::new(NapCatConfig::new(base_url, Duration::from_secs(2), None)?)
}

fn ok(body: Value) -> MockResponse {
    MockResponse {
        status: 200,
        body: body.to_string(),
        delay: Duration::ZERO,
    }
}

async fn mock_server(
    responses: Vec<MockResponse>,
) -> Result<(Url, usize), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let count = responses.len();
    tokio::spawn(async move {
        for response in responses {
            if serve_once(&listener, response).await.is_err() {
                break;
            }
        }
    });
    Ok((Url::parse(&format!("http://{address}/onebot/"))?, count))
}

async fn serve_once(listener: &TcpListener, response: MockResponse) -> Result<(), io::Error> {
    let (mut stream, _) = listener.accept().await?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "mock request ended before headers",
            ));
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    sleep(response.delay).await;
    let reason = if response.status >= 400 {
        "Error"
    } else {
        "OK"
    };
    let encoded = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        response.body.len(),
        response.body
    );
    stream.write_all(encoded.as_bytes()).await?;
    stream.shutdown().await
}
