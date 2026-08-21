use std::{error::Error, io, time::Duration};

use maimai_app::oauth::OAuthService;
use maimai_providers::{LxnsOAuthClient, OAuthConfig};
use maimai_storage::StateStore;
use rmcp::service::QuitReason;
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use url::Url;

use super::{OAuthDispatcher, PUBLIC_CONTRACT_JSON, oauth_server};
use crate::{DispatchError, ToolCall, ToolDispatcher};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

async fn token_server(responses: Vec<(u16, String)>) -> Result<Url, Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if read_request(&mut stream).await.is_err() {
                return;
            }
            let reason = if status >= 400 { "Error" } else { "OK" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    Ok(Url::parse(&format!("http://{address}/oauth/token"))?)
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<(), io::Error> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "missing headers",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok(())
}

fn client(token_url: Url) -> Result<LxnsOAuthClient, Box<dyn Error + Send + Sync>> {
    let config = OAuthConfig::new(
        "client-id",
        None,
        Some(Url::parse("https://bot.example.test/lxns/callback")?),
        Url::parse("https://maimai.example.test/oauth/authorize")?,
        token_url,
        vec!["read_player".to_owned()],
    )?;
    Ok(LxnsOAuthClient::new(config)?.with_timeout(Duration::from_secs(2))?)
}

async fn call(
    dispatcher: &OAuthDispatcher,
    name: &str,
    arguments: Value,
) -> Result<(String, Value), Box<dyn Error + Send + Sync>> {
    let Value::Object(arguments) = arguments else {
        return Err("arguments must be an object".into());
    };
    let output = dispatcher
        .dispatch(ToolCall::new(name.to_owned(), arguments))
        .await?;
    let (content, structured) = output.into_parts();
    let content = serde_json::to_value(content)?;
    let text = content[0]["text"]
        .as_str()
        .ok_or("text content missing")?
        .to_owned();
    Ok((text, structured.ok_or("structured content missing")?))
}

#[tokio::test]
async fn six_public_tools_keep_frozen_text_shapes_and_hide_secrets() -> TestResult {
    let success = r#"{"success":true,"data":{"access_token":"access-secret-sentinel","refresh_token":"refresh-secret-sentinel","expires_in":900}}"#;
    let secret_error = r#"{"error":"invalid_grant","error_description":"oauth-code-sentinel"}"#;
    let token_url = token_server(vec![
        (200, success.to_owned()),
        (200, success.to_owned()),
        (400, secret_error.to_owned()),
    ])
    .await?;
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let dispatcher = OAuthDispatcher::new(OAuthService::new(store.clone(), client(token_url)?));
    let context = json!({
        "subject": "subject-1",
        "adapter": "napcat",
        "conversation": "group:10001",
        "bot": "bot:20002",
    });

    let (url_text, url) = call(
        &dispatcher,
        "maimai_lxns_oauth_url",
        json!({
            "subject": "subject-1",
            "adapter": "napcat",
            "conversation": "group:10001",
            "bot": "bot:20002",
            "state": "signed.opaque.state",
        }),
    )
    .await?;
    assert_eq!(url["ok"], true);
    assert!(url.get("state").is_none());
    assert!(
        url["expiresAt"]
            .as_str()
            .is_some_and(|value| value.ends_with("+00:00"))
    );
    let authorization_url = url["authorizationUrl"]
        .as_str()
        .ok_or("authorization URL missing")?;
    assert_eq!(
        url_text,
        format!(
            "打开下面的落雪授权链接完成授权：\n{authorization_url}\n授权完成后提交回调 code；拍一拍流程还需要原上下文确认。"
        )
    );

    let mut prepare = context.clone();
    prepare["code"] = json!("oauth-code-sentinel");
    let (text, pending) = call(&dispatcher, "maimai_lxns_prepare_poke", prepare).await?;
    assert_eq!(
        text,
        "已收到落雪授权，等待原用户在原会话拍一拍当前机器人确认。"
    );
    assert_eq!(pending["ok"], true);
    assert_eq!(pending["pending"], true);
    assert!(pending["confirmationExpiresAt"].is_string());

    let (text, confirmed) = call(&dispatcher, "maimai_lxns_confirm_poke", context.clone()).await?;
    assert_eq!(text, "落雪 OAuth 已确认绑定。");
    assert_eq!(confirmed["confirmed"], true);
    assert_eq!(confirmed["status"], "confirmed");
    assert_eq!(confirmed["revision"], 1);

    let (text, status) = call(
        &dispatcher,
        "maimai_lxns_status",
        json!({"subject": "subject-1"}),
    )
    .await?;
    assert_eq!(text, "落雪 OAuth：已绑定。");
    assert_eq!(status["bound"], true);
    assert_eq!(status["pending"], false);

    let (text, unbound) = call(
        &dispatcher,
        "maimai_lxns_unbind",
        json!({"subject": "subject-1"}),
    )
    .await?;
    assert_eq!(text, "落雪 OAuth 已解绑。");
    assert_eq!(
        unbound,
        json!({"ok": true, "changed": true, "bound": false, "pending": false})
    );

    call(
        &dispatcher,
        "maimai_lxns_oauth_url",
        json!({"qq": "subject-2"}),
    )
    .await?;
    let (text, bound) = call(
        &dispatcher,
        "maimai_lxns_bind_code",
        json!({"qq": "subject-2", "code": "second-code"}),
    )
    .await?;
    assert_eq!(text, "落雪 OAuth 已绑定。");
    assert_eq!(bound["bound"], true);
    assert_eq!(bound["hasRefreshToken"], true);

    call(
        &dispatcher,
        "maimai_lxns_oauth_url",
        json!({"qq": "subject-3"}),
    )
    .await?;
    let error = dispatcher
        .dispatch(ToolCall::new(
            "maimai_lxns_bind_code".to_owned(),
            Map::from_iter([
                ("qq".to_owned(), json!("subject-3")),
                ("code".to_owned(), json!("oauth-code-sentinel")),
            ]),
        ))
        .await
        .err()
        .ok_or("provider error missing")?;
    let DispatchError::Tool(failure) = error else {
        return Err("expected tool-level provider error".into());
    };
    let (content, structured) = failure.into_parts();
    let rendered = serde_json::to_string(&json!({
        "content": content,
        "structuredContent": structured,
    }))?;
    assert!(rendered.contains("INVALID_GRANT"));
    assert!(!rendered.contains("oauth-code-sentinel"));
    assert!(!rendered.contains("access-secret-sentinel"));
    assert!(!rendered.contains("refresh-secret-sentinel"));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn missing_client_keeps_local_tools_and_returns_config_missing() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let dispatcher = OAuthDispatcher::new(OAuthService::without_client(store.clone()));

    let (_, status) = call(
        &dispatcher,
        "maimai_lxns_status",
        json!({"qq": "subject-1"}),
    )
    .await?;
    assert_eq!(status["bound"], false);
    let (_, unbound) = call(
        &dispatcher,
        "maimai_lxns_unbind",
        json!({"qq": "subject-1"}),
    )
    .await?;
    assert_eq!(unbound["changed"], false);

    let error = dispatcher
        .dispatch(ToolCall::new(
            "maimai_lxns_oauth_url".to_owned(),
            Map::from_iter([("qq".to_owned(), json!("subject-1"))]),
        ))
        .await
        .err()
        .ok_or("missing OAuth configuration was accepted")?;
    let DispatchError::Tool(failure) = error else {
        return Err("expected tool-level configuration error".into());
    };
    let (content, structured) = failure.into_parts();
    let rendered = json!({
        "content": content,
        "structuredContent": structured,
    });
    assert_eq!(
        rendered["structuredContent"]["error"]["code"],
        "CONFIG_MISSING"
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn main_accepts_frozen_exchange_timeout_while_public_still_rejects_it() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let service = OAuthService::without_client(store.clone());
    let main = OAuthDispatcher::main(service.clone());
    let public = OAuthDispatcher::new(service);

    let bind = Map::from_iter([
        ("qq".to_owned(), json!("subject-1")),
        ("code".to_owned(), json!("oauth-code-sentinel")),
        ("timeout".to_owned(), json!(0.5)),
    ]);
    assert_eq!(
        failure_code(
            main.dispatch(ToolCall::new(
                "maimai_lxns_bind_code".to_owned(),
                bind.clone(),
            ))
            .await
        )?,
        "CONFIG_MISSING"
    );
    assert_eq!(
        failure_code(
            public
                .dispatch(ToolCall::new("maimai_lxns_bind_code".to_owned(), bind,))
                .await
        )?,
        "INVALID_INPUT"
    );

    let prepare = Map::from_iter([
        ("qq".to_owned(), json!("subject-1")),
        ("code".to_owned(), json!("oauth-code-sentinel")),
        ("state".to_owned(), json!("signed.opaque.state")),
        ("adapterId".to_owned(), json!("napcat")),
        ("groupId".to_owned(), json!("group:10001")),
        ("botQq".to_owned(), json!("bot:20002")),
        ("timeout".to_owned(), json!(1.0)),
    ]);
    assert_eq!(
        failure_code(
            main.dispatch(ToolCall::new(
                "maimai_lxns_prepare_poke".to_owned(),
                prepare,
            ))
            .await
        )?,
        "CONFIG_MISSING"
    );
    let invalid = Map::from_iter([
        ("qq".to_owned(), json!("subject-1")),
        ("code".to_owned(), json!("oauth-code-sentinel")),
        ("timeout".to_owned(), json!(301)),
    ]);
    assert_eq!(
        failure_code(
            main.dispatch(ToolCall::new("maimai_lxns_bind_code".to_owned(), invalid,))
                .await
        )?,
        "INVALID_INPUT"
    );
    store.close().await;
    Ok(())
}

fn failure_code(
    result: Result<crate::ToolOutput, DispatchError>,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let DispatchError::Tool(failure) = result.err().ok_or("tool failure missing")? else {
        return Err("expected tool-level failure".into());
    };
    let (_, structured) = failure.into_parts();
    structured
        .and_then(|value| value["error"]["code"].as_str().map(str::to_owned))
        .ok_or_else(|| "structured error code missing".into())
}

#[tokio::test]
async fn duplex_server_lists_exact_contract_and_calls_status() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let token_url = Url::parse("http://127.0.0.1:9/oauth/token")?;
    let server = oauth_server(
        PUBLIC_CONTRACT_JSON,
        OAuthDispatcher::new(OAuthService::new(store.clone(), client(token_url)?)),
    )?;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
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
    let (client_read, mut client_write) = tokio::io::split(client_io);
    let mut lines = BufReader::new(client_read).lines();
    write_message(
        &mut client_write,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "oauth-test", "version": "1.0.0"}
            }
        }),
    )
    .await?;
    let initialized = read_message(&mut lines).await?;
    assert_eq!(initialized["result"]["serverInfo"]["name"], "lxns-oauth");
    write_message(
        &mut client_write,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await?;
    write_message(
        &mut client_write,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    )
    .await?;
    let listed = read_message(&mut lines).await?;
    assert_eq!(listed["result"]["tools"].as_array().map(Vec::len), Some(6));
    assert_eq!(
        listed["result"]["tools"][0]["name"],
        "maimai_lxns_oauth_url"
    );
    write_message(
        &mut client_write,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "maimai_lxns_status", "arguments": {"qq": "subject-1"}}
        }),
    )
    .await?;
    let called = read_message(&mut lines).await?;
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(called["result"]["structuredContent"]["bound"], false);
    client_write.shutdown().await?;
    drop(client_write);
    server_task.await??;
    store.close().await;
    Ok(())
}

async fn write_message<W>(writer: &mut W, message: Value) -> Result<(), io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut encoded = serde_json::to_vec(&message).map_err(io::Error::other)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await
}

async fn read_message<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<Value, io::Error>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "MCP response missing"))?;
    serde_json::from_str(&line).map_err(io::Error::other)
}
