use std::error::Error;

use maimai_core::{Difficulty, PlayerSelector, PlayerUsername, QqId, SongIdValue};
use secrecy::SecretString;
use serde_json::{Value, json};

use crate::{DivingFishClient, DivingFishCredentials, ProviderErrorCode};

use super::{
    DivingFishChartGeneration, DivingFishScoreClient, DivingFishScoreErrorCode, PlateVersions,
    mock::{MockResponse, cover_base, keep_alive_server, server},
};

fn client(api_base: &str) -> Result<DivingFishScoreClient, crate::ProviderError> {
    Ok(DivingFishScoreClient::new(
        DivingFishClient::with_base_urls(api_base, &cover_base(api_base))?,
    ))
}

fn qq(value: &str) -> Result<PlayerSelector, maimai_core::ValidationError> {
    Ok(PlayerSelector::Qq(QqId::new(value)?))
}

#[tokio::test]
async fn query_b50_matches_legacy_body_and_exact_decimal_normalization()
-> Result<(), Box<dyn Error>> {
    let (api_base, captured) = server(MockResponse {
        status: 200,
        body: json!({
            "nickname": "Tester",
            "username": "waterfish-user",
            "rating": "620",
            "additional_rating": 10,
            "plate": "Plate",
            "charts": {
                "sd": [{
                    "song_id": "1", "title": "Static Song", "type": "SD",
                    "level": "13", "level_label": "Master", "level_index": "3",
                    "ds": "13.4000", "achievements": 100.1234, "dx_score": "1234",
                    "ra": "300", "rate": "sss", "fc": "fc", "fs": "fs"
                }],
                "dx": [{
                    "songId": 10002, "title": "DX Song", "type": "dx",
                    "level": "13+", "levelLabel": "Master", "levelIndex": 3,
                    "ds": 13.8, "achievements": "100.5678", "dxScore": 2345,
                    "ra": 320, "rate": "sssp", "fc": "fcp", "fs": "fsdp"
                }]
            }
        })
        .to_string(),
    })
    .await?;
    let result = client(&api_base)?.query_b50(qq("10001")?).await?;
    let request = captured.await?;
    assert_eq!(
        request.lines().next(),
        Some("POST /api/maimaidxprober/query/player HTTP/1.1")
    );
    assert_eq!(request_json(&request)?, json!({"qq": "10001", "b50": "1"}));
    assert_eq!(result.counts.total, 2);
    assert_eq!(result.rating_breakdown.total, 620);
    assert_eq!(result.player.rating, Some(620));
    assert_eq!(result.sd[0].generation, DivingFishChartGeneration::Standard);
    assert_eq!(result.dx[0].generation, DivingFishChartGeneration::Deluxe);
    assert_eq!(result.sd[0].difficulty, Difficulty::Master);
    assert_eq!(
        result.sd[0]
            .constant
            .map(|value| value.value().normalize().to_string()),
        Some("13.4".to_owned())
    );
    assert_eq!(
        result.dx[0]
            .achievements
            .map(|value| value.ten_thousandths()),
        Some(1_005_678)
    );
    assert!(matches!(
        result.dx[0].song_id.value(),
        SongIdValue::Numeric(10_002)
    ));
    Ok(())
}

#[test]
fn selector_requires_exactly_one_typed_identity() -> Result<(), Box<dyn Error>> {
    let qq = QqId::new("10001")?;
    let username = PlayerUsername::new("tester")?;
    assert!(DivingFishScoreClient::selector(None, None).is_err());
    assert!(DivingFishScoreClient::selector(Some(qq.clone()), Some(username.clone())).is_err());
    assert_eq!(
        DivingFishScoreClient::selector(Some(qq), None)?,
        PlayerSelector::Qq(QqId::new("10001")?)
    );
    assert_eq!(
        DivingFishScoreClient::selector(None, Some(username))?,
        PlayerSelector::Username(PlayerUsername::new("tester")?)
    );
    Ok(())
}

#[tokio::test]
async fn query_plate_accepts_verlist_and_typed_utage() -> Result<(), Box<dyn Error>> {
    let (api_base, captured) = server(MockResponse {
        status: 200,
        body: json!({
            "nickname": "Plate User",
            "verlist": [{
                "music_id": "90001", "title": "Utage Song", "type": "UTAGE",
                "level": "宴", "difficulty": "Utage", "achievements": "153.5756",
                "ra": "0"
            }]
        })
        .to_string(),
    })
    .await?;
    let versions = PlateVersions::new(["FESTiVAL PLUS".to_owned()])?;
    let selector = PlayerSelector::Username(PlayerUsername::new("tester")?);
    let result = client(&api_base)?.query_plate(selector, &versions).await?;
    let request = captured.await?;
    assert_eq!(
        request.lines().next(),
        Some("POST /api/maimaidxprober/query/plate HTTP/1.1")
    );
    assert_eq!(
        request_json(&request)?,
        json!({"username": "tester", "version": ["FESTiVAL PLUS"]})
    );
    assert_eq!(
        result.records[0].generation,
        DivingFishChartGeneration::Utage
    );
    assert_eq!(result.records[0].difficulty, Difficulty::Utage);
    assert_eq!(result.records[0].generation.ranked_generation(), None);
    assert_eq!(
        result.records[0]
            .achievements
            .and_then(|value| value.utage())
            .map(maimai_core::UtageScore::ten_thousandths),
        Some(1_535_756)
    );
    Ok(())
}

#[tokio::test]
async fn developer_records_reuse_auth_client_and_normalize_snake_case() -> Result<(), Box<dyn Error>>
{
    let token = "developer-token-sentinel";
    let (api_base, captured) = server(MockResponse {
        status: 200,
        body: json!({
            "nickname": "Dev User", "rating": 15000,
            "records": [{
                "id": 8, "title": "True Love Song", "type": "SD", "level": "12",
                "level_index": 3, "ds": "12.7", "achievements": "99.5000",
                "dx_score": 1000, "ra": 200, "version": "maimai PLUS"
            }]
        })
        .to_string(),
    })
    .await?;
    let credentials = DivingFishCredentials::new().with_developer_token(SecretString::from(token));
    let result = client(&api_base)?
        .query_developer_records(qq("10001")?, credentials)
        .await?;
    let request = captured.await?;
    let lowered = request.to_ascii_lowercase();
    assert!(lowered.starts_with("get /api/maimaidxprober/dev/player/records?qq=10001"));
    assert!(lowered.contains("developer-token: developer-token-sentinel"));
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].version.as_deref(), Some("maimai PLUS"));
    assert!(!format!("{:?}", client(&api_base)?).contains(token));
    Ok(())
}

#[tokio::test]
async fn rating_ranking_accepts_legacy_names_and_numeric_strings() -> Result<(), Box<dyn Error>> {
    let (api_base, captured) = server(MockResponse {
        status: 200,
        body: json!([
            {"username": "middle", "ra": 15000},
            {"name": "tieA", "rating": "16000"}
        ])
        .to_string(),
    })
    .await?;
    let result = client(&api_base)?.rating_ranking().await?;
    let request = captured.await?;
    assert_eq!(
        request.lines().next(),
        Some("GET /api/maimaidxprober/rating_ranking HTTP/1.1")
    );
    assert_eq!(result[0].username.as_str(), "middle");
    assert_eq!(result[1].rating, 16_000);
    Ok(())
}

#[tokio::test]
async fn rating_ranking_rejects_the_entire_payload_when_any_row_is_malformed()
-> Result<(), Box<dyn Error>> {
    let (api_base, _captured) = server(MockResponse {
        status: 200,
        body: json!([
            {"username": "valid", "ra": 16000},
            {"username": "malformed", "ra": "not-a-number"}
        ])
        .to_string(),
    })
    .await?;
    let error = client(&api_base)?
        .rating_ranking()
        .await
        .err()
        .ok_or("malformed ranking should fail")?;
    assert_eq!(error.code(), DivingFishScoreErrorCode::InvalidResponse);
    Ok(())
}

#[tokio::test]
async fn cloned_score_clients_reuse_the_same_http_connection() -> Result<(), Box<dyn Error>> {
    let (api_base, mut captured) = keep_alive_server().await?;
    let original = client(&api_base)?;
    let cloned = original.clone();
    tokio::time::timeout(std::time::Duration::from_secs(1), original.rating_ranking()).await??;
    tokio::time::timeout(std::time::Duration::from_secs(1), cloned.rating_ranking()).await??;
    assert!(captured.recv().await.is_some());
    assert!(captured.recv().await.is_some());
    Ok(())
}

#[tokio::test]
async fn malformed_shapes_types_difficulties_and_numbers_fail_closed() -> Result<(), Box<dyn Error>>
{
    for score in [
        json!({"id": 1, "title": "Bad", "type": "UNKNOWN", "level": "1", "levelIndex": 0}),
        json!({"id": 1, "title": "Bad", "type": "SD", "level": "1", "difficulty": "Mystery"}),
        json!({"id": 1, "title": "Bad", "type": "DX", "level": "1", "levelIndex": 9}),
        json!({"id": 1, "title": "Bad", "type": "DX", "level": "1", "levelIndex": 0, "achievements": "101.0001"}),
        json!({"id": 1, "title": "Bad", "type": "DX", "level": "1", "levelIndex": 0, "fc": "clear"}),
        json!({"id": 1, "title": "Bad", "type": "DX", "level": "1", "levelIndex": 0, "fs": "unknown"}),
    ] {
        let (api_base, _captured) = server(MockResponse {
            status: 200,
            body: json!({"records": [score]}).to_string(),
        })
        .await?;
        let error = client(&api_base)?
            .query_developer_records(
                qq("10001")?,
                DivingFishCredentials::new().with_developer_token("token"),
            )
            .await
            .err()
            .ok_or("expected malformed response error")?;
        assert_eq!(error.code(), DivingFishScoreErrorCode::InvalidResponse);
    }
    Ok(())
}

#[tokio::test]
async fn provider_error_keeps_status_and_redacts_token_and_request_body()
-> Result<(), Box<dyn Error>> {
    let token = "developer-secret-sentinel";
    let qq = "99887766";
    let (api_base, _captured) = server(MockResponse {
        status: 403,
        body: json!({
            "message": format!("rejected {token} for {qq}"),
            "developer_token": "server-secret"
        })
        .to_string(),
    })
    .await?;
    let error = client(&api_base)?
        .query_developer_records(
            PlayerSelector::Qq(QqId::new(qq)?),
            DivingFishCredentials::new().with_developer_token(token),
        )
        .await
        .err()
        .ok_or("expected HTTP error")?;
    assert_eq!(
        error.code(),
        DivingFishScoreErrorCode::Provider(ProviderErrorCode::Http)
    );
    assert_eq!(error.status(), Some(403));
    let body = error.body().ok_or("sanitized body missing")?;
    assert!(!body.contains(token));
    assert!(!body.contains(qq));
    assert!(!body.contains("server-secret"));
    assert!(body.contains("[REDACTED]"));
    let debug = format!("{error:?}");
    assert!(!debug.contains(token) && !debug.contains(qq));
    Ok(())
}

fn request_json(request: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(request.split_once("\r\n\r\n").map_or("", |parts| parts.1))
}
