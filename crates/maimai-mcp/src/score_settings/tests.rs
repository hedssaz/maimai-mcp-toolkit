use std::{error::Error, io};

use maimai_app::score_settings::{AllowedScoreSources, ScoreSettingsService};
use maimai_core::ScoreSource;
use maimai_storage::StateStore;
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::{
    MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON, ScoreSettingsHandler, score_settings_server,
};
use crate::{DispatchError, ToolCall, ToolDispatcher};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

async fn call(
    handler: &ScoreSettingsHandler,
    name: &str,
    arguments: Value,
) -> Result<(String, Value), Box<dyn Error + Send + Sync>> {
    let Value::Object(arguments) = arguments else {
        return Err("arguments must be an object".into());
    };
    let output = handler
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
async fn five_handlers_match_text_shapes_share_storage_and_hide_token() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let handler = ScoreSettingsHandler::new(ScoreSettingsService::new(
        store.clone(),
        AllowedScoreSources::main(),
    ));
    let secret = "developer-token-secret-sentinel";
    let replacement_secret = "replacement-token-secret-sentinel";

    let (text, bound) = call(
        &handler,
        "bind_developer_token",
        json!({"developerToken": secret}),
    )
    .await?;
    assert_eq!(text, "Developer-Token 已绑定。");
    assert_eq!(bound["bound"], true);
    assert!(bound["updatedAt"].is_string());
    assert!(bound["securityNotice"].is_string());
    let serialized = bound.to_string();
    for forbidden in [secret, "tokenPreview", "storePath", "hash"] {
        assert!(!serialized.contains(forbidden));
    }
    let (text, rebound) = call(
        &handler,
        "bind_developer_token",
        json!({"developerToken": replacement_secret}),
    )
    .await?;
    assert_eq!(text, "Developer-Token 已绑定。");
    assert_eq!(rebound["bound"], true);
    assert!(!rebound.to_string().contains(replacement_secret));

    let (text, status) = call(&handler, "developer_token_status", json!({})).await?;
    let updated_at = status["updatedAt"].as_str().ok_or("updatedAt missing")?;
    let security_notice = status["securityNotice"]
        .as_str()
        .ok_or("securityNotice missing")?;
    assert_eq!(
        text,
        format!("Developer-Token 已绑定。\n更新时间：{updated_at}\n安全提示：{security_notice}")
    );
    assert!(!format!("{status:?}").contains(secret));

    let expected_source_text = "已切换成绩默认数据源：落雪。\n后续查询只会使用该数据源，不会自动切换到另一个数据源。\n通常使用水鱼；如果已完成落雪 OAuth，可切到落雪；如果不想绑定外部查分器，可以切到本地缓存。\n本地缓存需要先通过本机器人完成成绩导入。\n之后可发送 source sy、source lxns 或 source local 再切换。";
    let (text, switched) = call(
        &handler,
        "switch_score_source",
        json!({"qq": "10001", "source": "lxns"}),
    )
    .await?;
    assert_eq!(text, expected_source_text);
    assert_eq!(switched["preferredSource"], "lxns");
    assert_eq!(switched["sourceLabel"], "落雪");
    assert!(switched.get("preferenceFile").is_none());
    let (_, legacy) = call(
        &handler,
        "switch_b50_source",
        json!({"qq": "10001", "source": "divingfish"}),
    )
    .await?;
    assert_eq!(legacy["preferredSource"], "sy");
    let (_, defaulted) = call(&handler, "switch_b50_source", json!({"qq": "10002"})).await?;
    assert_eq!(defaulted["preferredSource"], "sy");
    assert_eq!(
        store
            .score_source_preference(&maimai_core::QqId::new("10001")?)
            .await?,
        Some(ScoreSource::DivingFish)
    );

    let (text, cleared) = call(&handler, "clear_developer_token", json!({})).await?;
    assert_eq!(text, "Developer-Token 已清除。");
    assert_eq!(cleared, json!({"bound": false, "cleared": true}));
    let (text, cleared) = call(&handler, "clear_developer_token", json!({})).await?;
    assert_eq!(text, "没有已绑定的 Developer-Token。");
    assert_eq!(cleared, json!({"bound": false, "cleared": false}));
    let (text, status) = call(&handler, "developer_token_status", json!({})).await?;
    assert_eq!(
        text,
        format!(
            "Developer-Token 未绑定。\n安全提示：{}",
            status["securityNotice"]
                .as_str()
                .ok_or("securityNotice missing")?
        )
    );
    assert_eq!(status["updatedAt"], Value::Null);
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn public_allowlist_rejects_lxns_as_a_tool_failure() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let handler = ScoreSettingsHandler::new(ScoreSettingsService::new(
        store.clone(),
        AllowedScoreSources::public(),
    ));
    let error = handler
        .dispatch(ToolCall::new(
            "switch_score_source".to_owned(),
            serde_json::Map::from_iter([
                ("qq".to_owned(), json!("10001")),
                ("source".to_owned(), json!("lxns")),
            ]),
        ))
        .await
        .err()
        .ok_or("public accepted lxns")?;
    let DispatchError::Tool(failure) = error else {
        return Err("expected tool failure".into());
    };
    let (_, structured) = failure.into_parts();
    assert_eq!(
        structured.ok_or("structured error missing")?["error"]["code"],
        "SOURCE_NOT_ALLOWED"
    );
    let (text, _) = call(
        &handler,
        "switch_score_source",
        json!({"qq": "10001", "source": "local"}),
    )
    .await?;
    assert!(!text.contains("lxns"));
    assert!(!text.contains("OAuth"));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn duplex_servers_retain_the_frozen_settings_surface() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let main = score_settings_server(
        MAIN_CONTRACT_JSON,
        ScoreSettingsHandler::new(ScoreSettingsService::new(
            store.clone(),
            AllowedScoreSources::main(),
        )),
    )?;
    assert_eq!(main.tools().len(), 5);
    let switch_schema = main
        .tools()
        .iter()
        .find(|tool| tool.name.as_ref() == "switch_score_source")
        .ok_or("main switch_score_source contract missing")?;
    assert_eq!(
        switch_schema.input_schema["properties"]["source"]["enum"],
        json!(["local", "sy", "lxns"])
    );
    let public = score_settings_server(
        PUBLIC_CONTRACT_JSON,
        ScoreSettingsHandler::new(ScoreSettingsService::new(
            store.clone(),
            AllowedScoreSources::public(),
        )),
    )?;
    assert_eq!(public.tools().len(), 3);

    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        let running = rmcp::serve_server(main, server_io)
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
                "clientInfo": {"name": "score-settings-test", "version": "1.0.0"}
            }
        }),
    )
    .await?;
    let _ = read_message(&mut lines).await?;
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
    assert_eq!(listed["result"]["tools"].as_array().map(Vec::len), Some(5));
    write_message(
        &mut client_write,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "developer_token_status", "arguments": {}}
        }),
    )
    .await?;
    let called = read_message(&mut lines).await?;
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(called["result"]["structuredContent"]["bound"], false);
    client_write.shutdown().await?;
    drop(client_write);
    task.await??;
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
