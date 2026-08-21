use std::{error::Error, fs, io, path::Path, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::ImageFormat;
use maimai_app::{
    b50_image::{
        B50ImageDataService, B50ImageService, B50ImageStyle, OutputPolicy, OutputStore,
        ResourceDirectories, StyleStore,
    },
    identity::IdentityDirectory,
    score_service::PlayerScoreService,
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_providers::{DivingFishClient, DivingFishScoreClient};
use maimai_render::{LegacyAssets, LegacyRenderer};
use maimai_storage::StateStore;
use rmcp::service::QuitReason;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

use super::{B50ImageDispatcher, b50_image_server};
use crate::{DispatchError, ToolCall};

fn fixed_now() -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp(1_768_435_200)
}

fn fixture_font() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf")
}

async fn fixture() -> Result<(TempDir, B50ImageDispatcher), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    write_catalog(root)?;
    let store = StateStore::open(root.join("state.db")).await?;
    let catalog = Arc::new(CatalogStore::load(CatalogFiles::from_data_dir(root)).await?);
    let provider =
        DivingFishClient::with_base_urls("http://127.0.0.1:9/api/", "http://127.0.0.1:9/covers/")?;
    let scores = Arc::new(PlayerScoreService::diving_fish_only(
        store.clone(),
        catalog,
        DivingFishScoreClient::new(provider),
    ));
    let data = Arc::new(B50ImageDataService::new(
        scores,
        IdentityDirectory::new(store),
    ));
    let font = fixture_font();
    let renderer = LegacyRenderer::new(LegacyAssets::new(&font, &font))?;
    let service = B50ImageService::new(
        StyleStore::new(root.join("state/style.json"), B50ImageStyle::Yuzu)?,
        OutputStore::new(root.join("images"), OutputPolicy::standard())?,
        renderer,
        ResourceDirectories::new(root.join("static"), root.join("covers"))?,
    );
    Ok((temporary, B50ImageDispatcher::new(service, data)))
}

#[tokio::test]
async fn b50_data_file_result_matches_structured_snapshot_and_decodes_png()
-> Result<(), Box<dyn Error>> {
    let (temporary, dispatcher) = fixture().await?;
    let output = dispatcher
        .dispatch_call_at(
            call(
                "render_b50_image",
                json!({"b50Data": fake_b50(), "style": "legacy"}),
            )?,
            fixed_now()?,
        )
        .await?;
    let (content, structured) = output.into_parts();
    let structured = structured.ok_or("structured content missing")?;
    let root = temporary.path().canonicalize()?;
    let image_path = root.join("images/10001_20260115T000000Z.png");
    let caption = "当前数据源：水鱼\n通常使用水鱼；如果已绑定落雪，可以发送 source lxns 切到落雪。\n如果不想绑定外部查分器，可以发送 source local 切到本地缓存。\n本地缓存需要先通过本机器人完成成绩导入。\n发送 source sy 可切回水鱼。";
    let text = format!(
        "B50 图片已生成。\n玩家: Test Player / Rating: 620\n图片: {}\n缺失曲绘: 2",
        image_path.display()
    );
    assert_eq!(content.len(), 1);
    assert_eq!(
        structured,
        json!({
            "imagePath": image_path,
            "mimeType": "image/png",
            "width": 1920,
            "height": 478,
            "outputMode": "file",
            "lookup": {"qq": "10001", "username": null},
            "player": {"nickname": "Test Player", "rating": 620, "plate": "Test Plate"},
            "counts": {"sd": 1, "dx": 1, "total": 2},
            "ratingBreakdown": {"sd": 300, "dx": 320, "total": 620},
            "style": "legacy",
            "missingCovers": [
                {"songId": 1, "title": "Static Song"},
                {"songId": 10002, "title": "DX Song"},
            ],
            "generatedAt": "2026-01-15T00:00:00.000000+00:00",
            "caption": caption,
            "text": text,
        })
    );
    let bytes = fs::read(
        structured["imagePath"]
            .as_str()
            .ok_or("image path missing")?,
    )?;
    let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png)?;
    assert_eq!((decoded.width(), decoded.height()), (1_920, 478));
    Ok(())
}

#[tokio::test]
async fn base64_mode_still_writes_file_and_request_style_wins() -> Result<(), Box<dyn Error>> {
    let (_temporary, dispatcher) = fixture().await?;
    let updated = dispatcher
        .dispatch_call_at(
            call("set_b50_image_style", json!({"style": "maibot"}))?,
            fixed_now()?,
        )
        .await?;
    let (_, updated) = updated.into_parts();
    let updated_text = "B50 图片默认风格已切换为: maibot";
    assert_eq!(
        updated.ok_or("style update missing")?,
        json!({
            "style": "maibot",
            "updatedAt": "2026-01-15T00:00:00.000000+00:00",
            "text": updated_text,
        })
    );
    let current = dispatcher
        .dispatch_call_at(call("get_b50_image_style", json!({}))?, fixed_now()?)
        .await?;
    let (_, current) = current.into_parts();
    assert_eq!(current.ok_or("style state missing")?["style"], "maibot");
    let output = dispatcher
        .dispatch_call_at(
            call(
                "render_b50_image",
                json!({
                    "b50Data": fake_b50(),
                    "style": "legacy",
                    "outputMode": "base64",
                }),
            )?,
            fixed_now()?,
        )
        .await?;
    let (_, structured) = output.into_parts();
    let structured = structured.ok_or("structured content missing")?;
    let path = Path::new(
        structured["imagePath"]
            .as_str()
            .ok_or("image path missing")?,
    );
    assert!(path.is_file());
    let encoded = structured["imageBase64"].as_str().ok_or("base64 missing")?;
    assert_eq!(STANDARD.decode(encoded)?, fs::read(path)?);
    assert_eq!(structured["style"], "legacy");

    let error = dispatcher
        .dispatch_call_at(
            call("render_b50_image", json!({"b50Data": fake_b50()}))?,
            fixed_now()?,
        )
        .await
        .err()
        .ok_or("expected persisted maibot to be unsupported")?;
    assert_eq!(tool_error_code(error), Some("MAIBOT_ASSETS_REQUIRED"));
    Ok(())
}

#[tokio::test]
async fn directory_overrides_are_limited_to_configured_roots() -> Result<(), Box<dyn Error>> {
    let (temporary, dispatcher) = fixture().await?;
    let root = temporary.path().canonicalize()?;
    let output_dir = root.join("images");
    let static_dir = root.join("static");
    let cover_dir = root.join("covers");
    let output = dispatcher
        .dispatch_call_at(
            call(
                "render_b50_image",
                json!({
                    "b50Data": fake_b50(),
                    "style": "legacy",
                    "staticDir": static_dir,
                    "outputDir": output_dir,
                    "coverCacheDir": cover_dir,
                }),
            )?,
            fixed_now()?,
        )
        .await?;
    let (_, structured) = output.into_parts();
    let structured = structured.ok_or("structured content missing")?;
    assert!(
        Path::new(
            structured["imagePath"]
                .as_str()
                .ok_or("image path missing")?
        )
        .starts_with(output_dir)
    );

    for field in ["outputDir", "staticDir", "coverCacheDir"] {
        let unconfigured = root.join(format!("unconfigured-{field}"));
        let mut arguments = json!({"b50Data": fake_b50(), "style": "legacy"});
        arguments[field] = json!(unconfigured);
        let error = dispatcher
            .dispatch_call_at(call("render_b50_image", arguments)?, fixed_now()?)
            .await
            .err()
            .ok_or("expected unconfigured directory override to fail")?;
        assert_eq!(tool_error_code(error), Some("RUNTIME_OVERRIDE_UNSUPPORTED"));
        assert!(!unconfigured.exists());
    }
    Ok(())
}

#[tokio::test]
async fn invalid_models_are_typed_tool_errors() -> Result<(), Box<dyn Error>> {
    let (_temporary, dispatcher) = fixture().await?;
    for (arguments, code) in [
        (json!({}), "INVALID_INPUT"),
        (json!({"qq": "1", "username": "name"}), "INVALID_INPUT"),
        (
            json!({"b50Data": fake_b50(), "style": "yuzu"}),
            "YUZU_ASSETS_REQUIRED",
        ),
    ] {
        let error = dispatcher
            .dispatch_call_at(call("render_b50_image", arguments)?, fixed_now()?)
            .await
            .err()
            .ok_or("expected tool error")?;
        assert_eq!(tool_error_code(error), Some(code));
    }

    let mut invalid_number = fake_b50();
    invalid_number["charts"]["sd"][0]["achievements"] = json!(100.12345);
    assert_error_code(&dispatcher, invalid_number, "INVALID_INPUT").await?;

    let mut control = fake_b50();
    control["player"]["nickname"] = json!("SENSITIVE_PLAYER\nname");
    let control_error = dispatcher
        .dispatch_call_at(
            call(
                "render_b50_image",
                json!({"b50Data": control, "style": "legacy"}),
            )?,
            fixed_now()?,
        )
        .await
        .err()
        .ok_or("expected control character error")?;
    assert!(!format!("{control_error:?}").contains("SENSITIVE_PLAYER"));
    assert_eq!(tool_error_code(control_error), Some("INVALID_INPUT"));

    let mut too_many = fake_b50();
    too_many["charts"]["sd"] = Value::Array(vec![fake_b50()["charts"]["sd"][0].clone(); 36]);
    assert_error_code(&dispatcher, too_many, "INVALID_INPUT").await?;
    Ok(())
}

#[tokio::test]
async fn render_semaphore_is_bounded_and_wait_uses_request_budget() -> Result<(), Box<dyn Error>> {
    let (temporary, dispatcher) = fixture().await?;
    let held = Arc::clone(&dispatcher.render_slots)
        .acquire_many_owned(2)
        .await?;
    assert_eq!(dispatcher.render_slots.available_permits(), 0);
    let error = dispatcher
        .dispatch_call_at(
            call(
                "render_b50_image",
                json!({"b50Data":fake_b50(),"style":"legacy","timeoutMs":1000}),
            )?,
            fixed_now()?,
        )
        .await
        .err()
        .ok_or("expected render permit timeout")?;
    assert_eq!(tool_error_code(error), Some("TIMEOUT"));
    let image_dir = temporary.path().join("images");
    assert!(
        !image_dir.exists()
            || fs::read_dir(image_dir)?.all(|entry| entry.ok().is_none_or(|entry| entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                != Some("png")))
    );
    drop(held);
    Ok(())
}

#[tokio::test]
async fn duplex_rmcp_lists_and_calls_the_three_tools() -> Result<(), Box<dyn Error>> {
    let (_temporary, dispatcher) = fixture().await?;
    let server = b50_image_server(dispatcher)?;
    let (client_io, server_io) = tokio::io::duplex(2 * 1024 * 1024);
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
    write_message(&mut client_write, &json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "fixture", "version": "1"}}
    })).await?;
    assert_eq!(
        read_message(&mut lines).await?["result"]["serverInfo"]["name"],
        "b50-image-mcp"
    );
    write_message(
        &mut client_write,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await?;
    write_message(
        &mut client_write,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await?;
    assert_eq!(
        read_message(&mut lines).await?["result"]["tools"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    write_message(
        &mut client_write,
        &json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"render_b50_image","arguments":{"b50Data":fake_b50(),"style":"legacy"}}
        }),
    )
    .await?;
    let called = read_message(&mut lines).await?;
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(
        called["result"]["structuredContent"]["mimeType"],
        "image/png"
    );
    drop(client_write);
    drop(lines);
    server_task.await??;
    Ok(())
}

async fn assert_error_code(
    dispatcher: &B50ImageDispatcher,
    b50: Value,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let error = dispatcher
        .dispatch_call_at(
            call(
                "render_b50_image",
                json!({"b50Data": b50, "style": "legacy"}),
            )?,
            fixed_now()?,
        )
        .await
        .err()
        .ok_or("expected tool error")?;
    assert_eq!(tool_error_code(error), Some(expected));
    Ok(())
}

fn tool_error_code(error: DispatchError) -> Option<&'static str> {
    let DispatchError::Tool(failure) = error else {
        return None;
    };
    let (_, structured) = failure.into_parts();
    match structured?.pointer("/error/code")?.as_str()? {
        "INVALID_INPUT" => Some("INVALID_INPUT"),
        "TIMEOUT" => Some("TIMEOUT"),
        "YUZU_ASSETS_REQUIRED" => Some("YUZU_ASSETS_REQUIRED"),
        "MAIBOT_ASSETS_REQUIRED" => Some("MAIBOT_ASSETS_REQUIRED"),
        "RUNTIME_OVERRIDE_UNSUPPORTED" => Some("RUNTIME_OVERRIDE_UNSUPPORTED"),
        _ => None,
    }
}

fn call(name: &str, arguments: Value) -> Result<ToolCall, Box<dyn Error>> {
    let arguments = arguments
        .as_object()
        .cloned()
        .ok_or("arguments must be object")?;
    Ok(ToolCall::new(name.to_owned(), arguments))
}

fn fake_b50() -> Value {
    json!({
        "lookup": {"qq": "10001"},
        "player": {"nickname": "Test Player", "rating": 620, "plate": "Test Plate"},
        "counts": {"sd": 1, "dx": 1, "total": 2},
        "ratingBreakdown": {"sd": 300, "dx": 320, "total": 620},
        "charts": {
            "sd": [{"songId": 1, "title": "Static Song", "type": "SD", "level": "13", "levelLabel": "Master", "ds": 13.4, "achievements": 100.1234, "ra": 300, "rate": "sss", "fc": "fc", "fs": "fs"}],
            "dx": [{"songId": 10002, "title": "DX Song", "type": "DX", "level": "13+", "levelLabel": "Master", "ds": 13.8, "achievements": 100.5678, "ra": 320, "rate": "sssp", "fc": "fcp", "fs": "fsdp"}],
        },
    })
}

fn write_catalog(root: &Path) -> Result<(), io::Error> {
    for (name, content) in [
        (
            "divingfish_song_list.json",
            r#"[{"id":"1","title":"Static Song","type":"SD","ds":[1,2,3,13.4],"level":["1","2","3","13"],"charts":[{},{},{},{}],"basic_info":{"artist":"A","genre":"maimai","bpm":150,"from":"Old","is_new":false}},{"id":"10002","title":"DX Song","type":"DX","ds":[1,2,3,13.8],"level":["1","2","3","13+"],"charts":[{},{},{},{}],"basic_info":{"artist":"A","genre":"maimai","bpm":150,"from":"Current","is_new":true}}]"#,
        ),
        (
            "lxns_song_list.json",
            r#"{"songs":[],"genres":[],"versions":[]}"#,
        ),
        ("lxns_alias_list.json", r#"{"aliases":[]}"#),
        ("music_alias.json", r#"{"content":[]}"#),
        ("custom_aliases.json", "{}"),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#),
        ("zh_s2t.json", "{}"),
    ] {
        fs::write(root.join(name), content)?;
    }
    Ok(())
}

async fn write_message<W>(writer: &mut W, message: &Value) -> Result<(), io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut encoded = serde_json::to_vec(message).map_err(io::Error::other)?;
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
