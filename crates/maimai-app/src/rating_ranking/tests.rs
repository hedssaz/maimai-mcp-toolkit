use std::{error::Error, fs, path::PathBuf};

use maimai_providers::{DivingFishClient, DivingFishScoreClient};
use maimai_render::RatingRankingRenderer;
use serde_json::json;
use tempfile::TempDir;
use time::{OffsetDateTime, UtcOffset};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};

use crate::image_output::{ImageOutputPolicy, ImageOutputStore};

use super::{RatingRankingRequest, RatingRankingService, RatingRankingTarget};

#[tokio::test]
async fn qq_resolves_b50_username_then_renders_public_ranking() -> Result<(), Box<dyn Error>> {
    let (api_base, mut captured) = response_server(vec![
        (
            200,
            json!({
                "username":"Straße", "rating":16000,
                "charts":{"sd":[],"dx":[]}
            })
            .to_string(),
        ),
        (
            200,
            json!([
                {"username":"lower","ra":15000},
                {"username":"STRASSE","ra":16000},
                {"username":"Straße","ra":16000}
            ])
            .to_string(),
        ),
    ])
    .await?;
    let temp = TempDir::new()?;
    let service = service(&temp, &api_base)?;
    let result = service
        .render(RatingRankingRequest {
            target: RatingRankingTarget::qq(maimai_core::QqId::new("00123")?),
            now: fixed_now()?,
        })
        .await?;

    assert!(result.image_path.is_file());
    assert!(result.width >= 160);
    assert!(result.height > 20);
    let first = captured.recv().await.ok_or("missing b50 request")?;
    let second = captured.recv().await.ok_or("missing ranking request")?;
    assert!(first.starts_with("POST /api/maimaidxprober/query/player HTTP/1.1"));
    assert!(first.contains("\"qq\":\"00123\""), "{first}");
    assert!(first.contains("\"b50\":\"1\""), "{first}");
    assert!(second.starts_with("GET /api/maimaidxprober/rating_ranking HTTP/1.1"));
    Ok(())
}

#[tokio::test]
async fn oversized_provider_username_fails_closed() -> Result<(), Box<dyn Error>> {
    let long_name = "x".repeat(super::MAX_RANKING_USERNAME_CHARS + 1);
    let (api_base, _) = response_server(vec![(
        200,
        json!([{"username":long_name,"ra":16000}]).to_string(),
    )])
    .await?;
    let temp = TempDir::new()?;
    let error = service(&temp, &api_base)?
        .render(RatingRankingRequest {
            target: RatingRankingTarget::page(1)?,
            now: fixed_now()?,
        })
        .await
        .err()
        .ok_or("long provider username should fail")?;
    assert!(
        matches!(error, super::RatingRankingError::InvalidProviderUsername),
        "{error:?}"
    );
    assert!(!format!("{error:?}").contains(&long_name));
    Ok(())
}

#[tokio::test]
async fn provider_error_preserves_status_and_redacts_sensitive_body() -> Result<(), Box<dyn Error>>
{
    let sentinel = "ranking-secret-sentinel";
    let (api_base, _) = response_server(vec![(
        500,
        json!({"access_token":sentinel,"message":"upstream failed"}).to_string(),
    )])
    .await?;
    let temp = TempDir::new()?;
    let error = service(&temp, &api_base)?
        .render(RatingRankingRequest {
            target: RatingRankingTarget::page(1)?,
            now: fixed_now()?,
        })
        .await
        .err()
        .ok_or("provider request should fail")?;
    assert_eq!(error.status(), Some(500), "{error:?}");
    assert!(!error.body().unwrap_or("").contains(sentinel));
    assert!(!format!("{error:?}").contains(sentinel));
    Ok(())
}

fn service(temp: &TempDir, api_base: &str) -> Result<RatingRankingService, Box<dyn Error>> {
    let static_root = temp.path().join("static");
    fs::create_dir(&static_root)?;
    fs::copy(
        workspace_root().join("crates/maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf"),
        static_root.join("ShangguMonoSC-Regular.otf"),
    )?;
    let provider = DivingFishScoreClient::new(DivingFishClient::with_base_urls(
        api_base,
        &api_base.replace("/api/", "/covers/"),
    )?);
    Ok(RatingRankingService::new(
        provider,
        RatingRankingRenderer::new(&static_root)?,
        ImageOutputStore::new(temp.path().join("images"), ImageOutputPolicy::standard())?,
        UtcOffset::from_hms(9, 0, 0)?,
    ))
}

async fn response_server(
    responses: Vec<(u16, String)>,
) -> Result<(String, mpsc::Receiver<String>), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, rx) = mpsc::channel(responses.len().max(1));
    tokio::spawn(async move {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let Ok(request) = read_request(&mut stream).await else {
                return;
            };
            let _ = tx.send(request).await;
            let reason = if status == 200 { "OK" } else { "Error" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            if stream.write_all(response.as_bytes()).await.is_err() {
                return;
            }
        }
    });
    Ok((format!("http://{address}/api/"), rx))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<String, std::io::Error> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(header_end) = find_bytes(&bytes, b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(str::trim)
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= header_end + 4 + length {
                break;
            }
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn fixed_now() -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp(1_700_000_000)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
