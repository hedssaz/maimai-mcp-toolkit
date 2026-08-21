use std::{error::Error, fs, io, path::PathBuf, sync::Arc};

use maimai_app::{
    image_output::{ImageOutputPolicy, ImageOutputStore},
    rating_ranking::RatingRankingService,
};
use maimai_mcp::render_tools::{
    MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON,
    rating_ranking::{RatingRankingDispatcher, TOOL_NAME, rating_ranking_server},
};
use maimai_providers::{DivingFishClient, DivingFishScoreClient};
use maimai_render::RatingRankingRenderer;
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::{OffsetDateTime, UtcOffset};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines},
    net::TcpListener,
};

#[tokio::test]
async fn main_and_public_duplex_render_text_json_without_structured_content()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let ranking = json!([
        {"username":"Bob","ra":15500},
        {"username":"Alice","ra":16000}
    ])
    .to_string();
    let api_base = response_server(vec![
        ranking.clone(),
        ranking.clone(),
        ranking.clone(),
        ranking,
    ])
    .await?;
    let temp = TempDir::new()?;
    let static_root = temp.path().join("static");
    fs::create_dir(&static_root)?;
    fs::copy(
        workspace_root().join("crates/maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf"),
        static_root.join("ShangguMonoSC-Regular.otf"),
    )?;
    let provider = DivingFishScoreClient::new(DivingFishClient::with_base_urls(
        &api_base,
        &api_base.replace("/api/", "/covers/"),
    )?);
    let service = Arc::new(RatingRankingService::new(
        provider,
        RatingRankingRenderer::new(&static_root)?,
        ImageOutputStore::new(temp.path().join("images"), ImageOutputPolicy::standard())?,
        UtcOffset::from_hms(9, 0, 0)?,
    ));

    for contract in [MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON] {
        let responses = duplex_calls(contract, Arc::clone(&service)).await?;
        for response in responses {
            assert_eq!(response["result"]["isError"], false);
            assert!(response["result"].get("structuredContent").is_none());
            let payload = content_json(&response)?;
            assert_eq!(payload["mimeType"], "image/png");
            assert!(payload["width"].as_u64().is_some_and(|value| value >= 160));
            assert!(payload["height"].as_u64().is_some_and(|value| value > 20));
            assert!(PathBuf::from(payload["imagePath"].as_str().ok_or("path missing")?).is_file());
        }
    }
    Ok(())
}

async fn duplex_calls(
    contract: &str,
    service: Arc<RatingRankingService>,
) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
    let server = rating_ranking_server(contract, RatingRankingDispatcher::new(service, fixed_now))?;
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
            "clientInfo":{"name":"rating-ranking-test","version":"1"}}),
    )
    .await?;
    let listed = rpc(&mut write, &mut lines, 2, "tools/list", json!({})).await?;
    assert_eq!(listed["result"]["tools"][0]["name"], TOOL_NAME);
    let by_name = rpc(
        &mut write,
        &mut lines,
        3,
        "tools/call",
        json!({"name":TOOL_NAME,"arguments":{"name":"alice","username":"ignored"}}),
    )
    .await?;
    let out_of_range = rpc(
        &mut write,
        &mut lines,
        4,
        "tools/call",
        json!({"name":TOOL_NAME,"arguments":{"startRank":50,"endRank":60}}),
    )
    .await?;
    write.shutdown().await?;
    drop(write);
    server_task.await??;
    Ok(vec![by_name, out_of_range])
}

async fn response_server(responses: Vec<String>) -> Result<String, Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        for body in responses {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if read_request(&mut stream).await.is_err() {
                return;
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            if stream.write_all(response.as_bytes()).await.is_err() {
                return;
            }
        }
    });
    Ok(format!("http://{address}/api/"))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<(), io::Error> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
    }
}

async fn rpc(
    write: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    lines: &mut Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let message = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    write
        .write_all(serde_json::to_string(&message)?.as_bytes())
        .await?;
    write.write_all(b"\n").await?;
    write.flush().await?;
    loop {
        let line = lines.next_line().await?.ok_or("server closed")?;
        let value: Value = serde_json::from_str(&line)?;
        if value["id"] == id {
            return Ok(value);
        }
    }
}

fn content_json(response: &Value) -> Result<Value, Box<dyn Error + Send + Sync>> {
    Ok(serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .ok_or("tool text missing")?,
    )?)
}

fn fixed_now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000)
        .map_or(OffsetDateTime::UNIX_EPOCH, |value| value)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
