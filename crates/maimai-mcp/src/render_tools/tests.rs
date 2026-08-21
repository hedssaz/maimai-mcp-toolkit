use std::{error::Error, fs, io, path::PathBuf, sync::Arc, time::Duration};

use maimai_app::{
    image_output::{ImageOutputPolicy, ImageOutputStore},
    music_info::{MusicInfoBatchResult, MusicInfoChartType, MusicInfoImage, MusicInfoService},
    oauth::OAuthService,
    score_service::PlayerScoreService,
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_render::MusicInfoRenderer;
use maimai_storage::StateStore;
use rmcp::service::QuitReason;
use serde_json::json;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use url::Url;

use super::{
    MAIN_CONTRACT_JSON, MusicInfoDispatcher, PUBLIC_CONTRACT_JSON, TOOL_NAMES, convert,
    dto::MusicInfoBatchArgs, format, music_info_server,
};

#[test]
fn frozen_main_and_public_surfaces_keep_both_music_info_tools() -> Result<(), Box<dyn Error>> {
    for contract in [MAIN_CONTRACT_JSON, PUBLIC_CONTRACT_JSON] {
        let contract = crate::contract::SurfaceContract::parse(contract)?.retain_tools(&TOOL_NAMES);
        assert_eq!(
            contract
                .tools()
                .iter()
                .map(|tool| tool.name())
                .collect::<Vec<_>>(),
            TOOL_NAMES
        );
    }
    Ok(())
}

#[test]
fn batch_rejects_null_items_and_keeps_legacy_numeric_queries() -> Result<(), Box<dyn Error>> {
    let null: MusicInfoBatchArgs = serde_json::from_value(json!({"items":[null]}))?;
    let error = convert::batch(null)
        .err()
        .ok_or("null batch item should fail")?;
    assert_eq!(error.to_string(), "items[1] 必须是字符串、数字或曲目对象");

    let number: MusicInfoBatchArgs = serde_json::from_value(json!({"items":[383]}))?;
    let (requests, player) = convert::batch(number)?;
    assert!(player.is_none());
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].query_label(), "383");
    Ok(())
}

#[test]
fn single_and_batch_payloads_keep_legacy_json_shapes() -> Result<(), Box<dyn Error>> {
    let image = MusicInfoImage {
        index: 2,
        sub_index: 1,
        query: "Believe".to_owned(),
        music_id: "10835".to_owned(),
        title: "Believe the Rainbow".to_owned(),
        chart_type: Some(MusicInfoChartType::Deluxe),
        image_path: PathBuf::from("/tmp/music.png"),
        width: 1_200,
        height: 1_300,
    };
    let single = MusicInfoBatchResult {
        images: vec![image.clone()],
        errors: Vec::new(),
    };
    assert_eq!(
        format::single(&single)?,
        r#"{"imagePath":"/tmp/music.png","mimeType":"image/png","width":1200,"height":1300}"#
    );
    let batch: serde_json::Value = serde_json::from_str(&format::batch(&single)?)?;
    assert_eq!(batch["images"][0]["index"], 2);
    assert_eq!(batch["images"][0]["subIndex"], 1);
    assert_eq!(batch["images"][0]["chartType"], "DX");
    assert_eq!(batch["results"], batch["images"]);
    Ok(())
}

#[tokio::test]
async fn real_duplex_renders_dual_batch_and_cover_only_without_structured_content()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let catalog = Arc::new(
        CatalogStore::load(CatalogFiles::from_data_dir(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        ))
        .await?,
    );
    let state = StateStore::open(temp.path().join("state.db")).await?;
    let base = Url::parse("http://127.0.0.1:9/")?;
    let diving_fish = DivingFishClient::with_base_urls(
        base.join("api/")?.as_str(),
        base.join("covers/")?.as_str(),
    )?;
    let oauth = OAuthService::new(
        state.clone(),
        LxnsOAuthClient::new(OAuthConfig::new(
            "client-id",
            None,
            None,
            base.join("authorize")?,
            base.join("token")?,
            vec!["read_player".to_owned()],
        )?)?,
    );
    let scores = Arc::new(PlayerScoreService::with_lxns(
        state,
        Arc::clone(&catalog),
        DivingFishScoreClient::new(diving_fish),
        oauth,
        LxnsScoreEndpoint::new(base.join("lxns/")?, Duration::from_millis(100))?,
    ));
    let static_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../maimaidx_render_mcp/static");
    let cache = temp.path().join("cover-cache");
    fs::create_dir(&cache)?;
    image::RgbaImage::from_pixel(24, 24, image::Rgba([20, 40, 60, 255]))
        .save(cache.join("local-cover.png"))?;
    let service = Arc::new(MusicInfoService::new(
        catalog,
        scores,
        MusicInfoRenderer::new(static_root, &cache)?,
        ImageOutputStore::new(temp.path().join("images"), ImageOutputPolicy::standard())?,
    ));
    let server = music_info_server(MAIN_CONTRACT_JSON, MusicInfoDispatcher::new(service))?;
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
            "clientInfo":{"name":"render-test","version":"1"}}),
    )
    .await?;

    let dual = tool_call(
        &mut write,
        &mut lines,
        2,
        "render_maimai_music_info",
        json!({"query":"相信彩虹","qq":"123456"}),
    )
    .await?;
    assert_eq!(dual["result"]["isError"], false);
    assert!(dual["result"].get("structuredContent").is_none());
    let dual_value = content_json(&dual)?;
    assert_eq!(
        dual_value["images"]
            .as_array()
            .ok_or("dual images missing")?
            .iter()
            .map(|image| image["chartType"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        ["ST", "DX"]
    );
    for image in dual_value["images"].as_array().ok_or("images missing")? {
        assert_eq!(
            (image["width"].as_u64(), image["height"].as_u64()),
            (Some(1_200), Some(1_300))
        );
        assert!(PathBuf::from(image["imagePath"].as_str().ok_or("path missing")?).is_file());
    }

    let batch = tool_call(
        &mut write,
        &mut lines,
        3,
        "render_maimai_music_info_batch",
        json!({"queries":["MEGATON BLAST","definitely-not-a-song"]}),
    )
    .await?;
    assert_eq!(batch["result"]["isError"], false);
    let batch_value = content_json(&batch)?;
    assert_eq!(batch_value["images"][0]["index"], 1);
    assert_eq!(batch_value["errors"][0]["index"], "2");

    let partial = tool_call(
        &mut write,
        &mut lines,
        4,
        "render_maimai_music_info",
        json!({"image_name":"local-cover","knownTitle":"Local only"}),
    )
    .await?;
    assert_eq!(partial["result"]["isError"], false);
    let partial_value = content_json(&partial)?;
    assert_eq!(partial_value["width"], 1_200);
    assert!(PathBuf::from(partial_value["imagePath"].as_str().ok_or("partial path")?).is_file());

    let too_many = tool_call(
        &mut write,
        &mut lines,
        5,
        "render_maimai_music_info_batch",
        json!({"queries":vec!["Believe the Rainbow"; 51]}),
    )
    .await?;
    assert_eq!(too_many["result"]["isError"], true);
    assert_eq!(
        too_many["result"]["content"][0]["text"],
        "批量曲目信息最多支持 50 项"
    );

    write.shutdown().await?;
    drop(write);
    server_task.await??;
    Ok(())
}

fn content_json(message: &serde_json::Value) -> Result<serde_json::Value, Box<dyn Error>> {
    Ok(serde_json::from_str(
        message["result"]["content"][0]["text"]
            .as_str()
            .ok_or("tool text missing")?,
    )?)
}

async fn tool_call<W, R>(
    writer: &mut W,
    lines: &mut Lines<BufReader<R>>,
    id: u64,
    name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: tokio::io::AsyncRead + Unpin,
{
    rpc(
        writer,
        lines,
        id,
        "tools/call",
        json!({"name":name,"arguments":arguments}),
    )
    .await
}

async fn rpc<W, R>(
    writer: &mut W,
    lines: &mut Lines<BufReader<R>>,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: tokio::io::AsyncRead + Unpin,
{
    let mut message = serde_json::to_vec(&json!({
        "jsonrpc":"2.0","id":id,"method":method,"params":params
    }))
    .map_err(io::Error::other)?;
    message.push(b'\n');
    writer.write_all(&message).await?;
    writer.flush().await?;
    let line = lines
        .next_line()
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "response missing"))?;
    serde_json::from_str(&line).map_err(io::Error::other)
}
