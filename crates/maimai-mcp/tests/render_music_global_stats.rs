use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use maimai_app::{
    image_output::{ImageOutputPolicy, ImageOutputStore},
    music_global_stats::MusicGlobalStatsService,
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_mcp::render_tools::{
    MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON,
    global_stats::{MusicGlobalStatsDispatcher, TOOL_NAME, music_global_stats_server},
};
use maimai_render::MusicGlobalStatsRenderer;
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

#[tokio::test]
async fn main_and_public_duplex_keep_single_and_dual_text_json()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    let catalog = Arc::new(
        CatalogStore::load(CatalogFiles::from_data_dir(workspace_root().join("data"))).await?,
    );
    let static_root = temp.path().join("static");
    fs::create_dir(&static_root)?;
    let font = workspace_root().join("crates/maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf");
    fs::copy(&font, static_root.join("Torus SemiBold.otf"))?;
    fs::copy(&font, static_root.join("ResourceHanRoundedCN-Bold.ttf"))?;
    let service = Arc::new(MusicGlobalStatsService::new(
        catalog,
        MusicGlobalStatsRenderer::new(&static_root)?,
        ImageOutputStore::new(temp.path().join("images"), ImageOutputPolicy::standard())?,
    ));

    for contract in [MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON] {
        let responses = duplex_calls(contract, Arc::clone(&service)).await?;
        let dual = responses.first().ok_or("dual response missing")?;
        assert_eq!(dual["result"]["isError"], false);
        assert!(dual["result"].get("structuredContent").is_none());
        let payload = content_json(dual)?;
        let images = payload["images"].as_array().ok_or("images missing")?;
        assert_eq!(images.len(), 2);
        assert_eq!(images[0]["chartType"], "ST");
        assert_eq!(images[1]["chartType"], "DX");
        assert_eq!(images[0]["levelIndex"], 3);
        for image in images {
            assert_eq!(
                (image["width"].as_u64(), image["height"].as_u64()),
                (Some(1_000), Some(800))
            );
            assert!(PathBuf::from(image["imagePath"].as_str().ok_or("path missing")?).is_file());
        }

        let single = responses.get(1).ok_or("single response missing")?;
        assert_eq!(single["result"]["isError"], false);
        assert!(single["result"].get("structuredContent").is_none());
        let payload = content_json(single)?;
        assert_eq!(
            (payload["width"].as_u64(), payload["height"].as_u64()),
            (Some(1_000), Some(800))
        );
        assert!(payload.get("images").is_none());
    }
    Ok(())
}

async fn duplex_calls(
    contract: &str,
    service: Arc<MusicGlobalStatsService>,
) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
    let server = music_global_stats_server(contract, MusicGlobalStatsDispatcher::new(service))?;
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
            "clientInfo":{"name":"music-stats-test","version":"1"}}),
    )
    .await?;
    let listed = rpc(&mut write, &mut lines, 2, "tools/list", json!({})).await?;
    assert_eq!(listed["result"]["tools"][0]["name"], TOOL_NAME);
    let dual = rpc(
        &mut write,
        &mut lines,
        3,
        "tools/call",
        json!({"name":TOOL_NAME,"arguments":{"query":"Calamity Fortune","difficulty":"紫"}}),
    )
    .await?;
    let single = rpc(
        &mut write,
        &mut lines,
        4,
        "tools/call",
        json!({"name":TOOL_NAME,"arguments":{
            "music_id":"641","songType":"standard","difficulty":"Master"
        }}),
    )
    .await?;
    write.shutdown().await?;
    drop(write);
    server_task.await??;
    Ok(vec![dual, single])
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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
