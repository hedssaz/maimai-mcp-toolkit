use std::{error::Error, io, path::PathBuf, sync::Arc};

use maimai_app::{
    image_output::{ImageOutputPolicy, ImageOutputStore},
    score_list::ScoreListService,
    score_service::PlayerScoreService,
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_mcp::render_tools::{
    MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON,
    score_list::{ScoreListDispatcher, ScoreListSurface, TOOL_NAME, score_list_server},
};
use maimai_providers::{DivingFishClient, DivingFishScoreClient};
use maimai_render::ScoreListRenderer;
use maimai_storage::StateStore;
use rmcp::service::QuitReason;
use secrecy::SecretString;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines},
    net::TcpListener,
};

#[tokio::test]
async fn main_and_public_duplex_keep_contract_output_and_public_source_is_invalid()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    let api_base = response_server(2).await?;
    let catalog = Arc::new(
        CatalogStore::load(CatalogFiles::from_data_dir(workspace_root().join("data"))).await?,
    );
    let store = StateStore::open(temp.path().join("state.db")).await?;
    store
        .set_diving_fish_developer_token(
            &SecretString::from("developer-token"),
            OffsetDateTime::UNIX_EPOCH,
        )
        .await?;
    let scores = Arc::new(PlayerScoreService::diving_fish_only(
        store,
        Arc::clone(&catalog),
        DivingFishScoreClient::new(DivingFishClient::with_base_urls(
            &api_base,
            &api_base.replace("/api/", "/covers/"),
        )?),
    ));
    let service = Arc::new(ScoreListService::new(
        catalog,
        scores,
        ScoreListRenderer::new(
            workspace_root().join("maimaidx_render_mcp/static"),
            temp.path().join("covers"),
        )?,
        ImageOutputStore::new(temp.path().join("images"), ImageOutputPolicy::standard())?,
    ));

    let main = duplex_call(
        MAIN_CONTRACT_JSON,
        ScoreListSurface::Main,
        Arc::clone(&service),
        json!({"qq":"10001","level":"12","source":"sy"}),
    )
    .await?;
    assert_eq!(main["result"]["isError"], false, "{main}");
    assert!(main["result"].get("structuredContent").is_none());
    let main_payload = content_json(&main)?;
    assert_eq!(main_payload["scoreSource"], "sy");
    assert_eq!(main_payload["scoreSourceLabel"], "水鱼");
    assert!(main_payload["caption"].as_str().is_some());
    assert_image(&main_payload)?;

    let public = duplex_call(
        PUBLIC_CONTRACT_JSON,
        ScoreListSurface::Public,
        Arc::clone(&service),
        json!({"qq":"10001","rating":"12"}),
    )
    .await?;
    assert_eq!(public["result"]["isError"], false, "{public}");
    assert!(public["result"].get("structuredContent").is_none());
    let public_payload = content_json(&public)?;
    assert_eq!(
        public_payload.as_object().map(serde_json::Map::len),
        Some(4)
    );
    assert_image(&public_payload)?;

    let invalid = duplex_call(
        PUBLIC_CONTRACT_JSON,
        ScoreListSurface::Public,
        service,
        json!({"qq":"10001","level":"12","dataSource":null}),
    )
    .await?;
    assert_eq!(invalid["result"]["isError"], true);
    assert!(invalid["result"].get("structuredContent").is_none());
    assert!(
        invalid["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.starts_with("INVALID_INPUT:"))
    );
    Ok(())
}

async fn duplex_call(
    contract: &str,
    surface: ScoreListSurface,
    service: Arc<ScoreListService>,
    arguments: Value,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let server = score_list_server(
        contract,
        ScoreListDispatcher::new(service, surface, fixed_now),
    )?;
    let (client, server_io) = tokio::io::duplex(512 * 1024);
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
            "clientInfo":{"name":"score-list-test","version":"1"}}),
    )
    .await?;
    let listed = rpc(&mut write, &mut lines, 2, "tools/list", json!({})).await?;
    assert_eq!(listed["result"]["tools"][0]["name"], TOOL_NAME);
    let response = rpc(
        &mut write,
        &mut lines,
        3,
        "tools/call",
        json!({"name":TOOL_NAME,"arguments":arguments}),
    )
    .await?;
    write.shutdown().await?;
    drop(write);
    server_task.await??;
    Ok(response)
}

async fn response_server(count: usize) -> Result<String, Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let body = json!({
        "nickname":"Score List User","rating":15000,
        "records":[{
            "id":8,"title":"True Love Song","type":"SD","level":"12",
            "level_index":3,"ds":"12.4","achievements":"99.5000",
            "dx_score":850,"ra":200,"version":"maimai PLUS"
        }]
    })
    .to_string();
    tokio::spawn(async move {
        for _ in 0..count {
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
    let mut buffer = [0_u8; 1_024];
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

fn assert_image(payload: &Value) -> Result<(), Box<dyn Error + Send + Sync>> {
    assert_eq!(payload["mimeType"], "image/png");
    assert_eq!(payload["width"], 1_400);
    assert_eq!(payload["height"], 726);
    assert!(PathBuf::from(payload["imagePath"].as_str().ok_or("path missing")?).is_file());
    Ok(())
}

fn fixed_now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000)
        .map_or(OffsetDateTime::UNIX_EPOCH, |value| value)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
