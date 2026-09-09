use std::{error::Error, io, time::Duration};

use maimai_core::AchievementRate;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
    time::{sleep, timeout},
};
use url::Url;

use super::{
    CollectionRef, FriendCode, FullCombo, FullSync, LxnsChartType, LxnsDifficulty, LxnsScore,
    LxnsScoreClient, LxnsScoreConfig, LxnsScoreEndpoint, LxnsScoreErrorCode, LxnsSongId,
    MAX_RESPONSE_BODY_BYTES, PlayerUpdate, ScoreUpload,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

struct MockResponse {
    status: u16,
    body: String,
    delay: Duration,
    headers: Vec<(String, String)>,
}

#[test]
fn typed_ids_chart_types_and_camel_fields_follow_legacy_rules() -> TestResult {
    let friend = FriendCode::new("123 456_789-012")?;
    assert_eq!(friend.as_str(), "123456789012");
    assert!(FriendCode::new("12345").is_err());
    assert_eq!(
        serde_json::to_value(&friend)?,
        Value::Number(123_456_789_012_u64.into())
    );
    let decoded: FriendCode = serde_json::from_str(r#""123456789012""#)?;
    assert_eq!(decoded, friend);

    assert_eq!(LxnsSongId::for_query(10_008)?.get(), 8);
    assert_eq!(LxnsSongId::for_query(100_230)?.get(), 100_230);
    assert_eq!(
        LxnsSongId::from_waterfish(10_030, LxnsChartType::Deluxe)?.get(),
        30
    );
    assert_eq!(
        LxnsChartType::from_waterfish("DX", 100_230)?,
        LxnsChartType::Utage
    );

    let score: LxnsScore = serde_json::from_value(json!({
        "songId": 8,
        "chartType": "deluxe",
        "levelIndex": 3,
        "achievements": 100.1234,
        "dxScore": 1234,
        "dxRating": 321,
        "songName": "Fixture",
        "ds": "13.7",
        "fc": "app",
        "fs": "fsdp"
    }))?;
    assert_eq!(score.id.get(), 8);
    assert_eq!(score.chart_type, LxnsChartType::Deluxe);
    assert_eq!(score.level_index, LxnsDifficulty::Master);
    assert_eq!(score.achievements.ten_thousandths(), 1_001_234);
    assert_eq!(score.dx_score, 1_234);
    assert_eq!(score.dx_rating, Some(321));
    let current_bests_score: LxnsScore = serde_json::from_value(json!({
        "id": 8,
        "type": "dx",
        "level_index": 3,
        "achievements": 100.1234,
        "dx_score": 1234,
        "dx_rating": 324.1728
    }))?;
    assert_eq!(current_bests_score.dx_rating, None);
    assert!(
        serde_json::from_value::<LxnsScore>(json!({
            "id": 8, "type": "dx", "level_index": 5,
            "achievements": 100.0
        }))
        .is_err()
    );
    let utage: LxnsScore = serde_json::from_value(json!({
        "id": 111597,
        "type": "utage",
        "level_index": 0,
        "achievements": 153.5756,
        "dx_score": 2295
    }))?;
    assert_eq!(
        utage
            .achievements
            .utage()
            .map(maimai_core::UtageScore::ten_thousandths),
        Some(1_535_756)
    );
    assert!(
        serde_json::from_value::<LxnsScore>(json!({
            "id": 8,
            "type": "dx",
            "level_index": 0,
            "achievements": 153.5756
        }))
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn client_covers_read_update_upload_paths_queries_and_bearer() -> TestResult {
    let responses = vec![
        ok(json!({"success":true,"data":{
            "name":"tester","friend_code":123456789012345_u64,"rating":15000,
            "name_plate":{"id":4001,"name":"Rich Plate","color":"rainbow"}
        }})),
        ok(json!({"success":true,"data":{
            "player":{"name":"camel","friendCode":"123456789012345"},
            "scores":[{
                "songId":8,"chartType":"dx","levelIndex":1,
                "achievements":99.5,"dxScore":900,"dxRating":20,
                "songName":"True Love Song"
            }]
        }})),
        ok(json!({"success":true,"data":{
            "player":{"name":"bests"},
            "sd":[{
                "id":8,"type":"standard","level_index":0,
                "achievements":"100.0","dx_score":1000
            }],
            "new":[{
                "id":8,"type":"dx","level_index":1,
                "achievements":100.0,"dx_score":1100
            }],
            "b35Total":123,"dx_total":45
        }})),
        ok(json!({"success":true,"data":[{
            "id":8,"type":"dx","level_index":0,
            "achievements":100.0,"dx_score":1000
        }]})),
        ok(json!({"success":true,"data":{
            "name":"updated","friendCode":123456789012345_u64,"rating":16000
        }})),
        ok(json!({"success":true,"data":{"updated":1}})),
    ];
    let (base_url, mut requests) = mock_server(responses).await?;
    let client = client(base_url, Duration::from_secs(2), "access-token")?;
    let empty_error = client
        .upload_scores(&[])
        .await
        .err()
        .ok_or_else(|| io::Error::other("empty upload should fail"))?;
    assert_eq!(empty_error.code(), LxnsScoreErrorCode::InvalidRequest);

    let player = client.player().await?;
    assert_eq!(player.name.as_deref(), Some("tester"));
    assert_eq!(
        player.friend_code.as_ref().map(FriendCode::as_str),
        Some("123456789012345")
    );
    assert_eq!(
        player.name_plate.as_ref().and_then(CollectionRef::name),
        Some("Rich Plate")
    );
    assert_eq!(
        player.name_plate.as_ref().and_then(CollectionRef::color),
        Some("rainbow")
    );
    let scores = client.scores().await?;
    assert_eq!(scores.scores.len(), 1);
    assert_eq!(scores.scores[0].chart_type, LxnsChartType::Deluxe);
    let bests = client.bests().await?;
    assert_eq!(bests.standard_total, Some(123));
    assert_eq!(bests.deluxe_total, Some(45));
    assert_eq!(bests.standard.len(), 1);
    assert_eq!(bests.deluxe.len(), 1);
    let song = client.song_bests(LxnsSongId::new(10_008)?).await?;
    assert_eq!(song.song_id.get(), 8);
    assert_eq!(song.scores.len(), 1);

    let update = PlayerUpdate {
        name: "updated".to_owned(),
        rating: 16_000,
        friend_code: FriendCode::new("123456789012345")?,
        course_rank: 7,
        class_rank: 8,
        star: 9,
        trophy: None,
        icon: None,
        name_plate: player.name_plate.clone(),
        frame: None,
    };
    assert_eq!(client.update_player(&update).await?.rating, Some(16_000));
    let uploads = [
        ScoreUpload::new(
            LxnsSongId::new(8)?,
            LxnsChartType::Standard,
            LxnsDifficulty::Basic,
            AchievementRate::from_decimal_str("100.1234")?.into(),
            Some(FullCombo::Fc),
            Some(FullSync::Fs),
            1_000,
        )?,
        ScoreUpload::new(
            LxnsSongId::new(111_597)?,
            LxnsChartType::Utage,
            LxnsDifficulty::Basic,
            maimai_core::UtageScore::from_ten_thousandths(1_535_756).into(),
            None,
            None,
            2_295,
        )?,
    ];
    let receipt = client.upload_scores(&uploads).await?;
    assert_eq!(receipt.uploaded, 2);
    assert_eq!(receipt.updated, Some(1));

    let captured = drain_requests(&mut requests).await;
    assert_eq!(captured.len(), 6);
    assert!(captured.iter().all(|request| {
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer access-token")
    }));
    assert!(captured[0].starts_with("GET /api/v0/user/maimai/player HTTP/1.1"));
    assert!(captured[1].starts_with("GET /api/v0/user/maimai/player/scores HTTP/1.1"));
    assert!(captured[2].starts_with("GET /api/v0/user/maimai/player/bests HTTP/1.1"));
    assert!(captured[3].starts_with("GET /api/v0/user/maimai/player/bests?song_id=8 HTTP/1.1"));
    assert!(captured[4].starts_with("PUT /api/v0/user/maimai/player HTTP/1.1"));
    let update_body = request_body(&captured[4])?;
    assert_eq!(update_body["friend_code"], 123_456_789_012_345_u64);
    assert_eq!(update_body["name_plate"], json!({"id":4001}));
    assert!(captured[5].starts_with("POST /api/v0/user/maimai/player/scores HTTP/1.1"));
    let upload_body = request_body(&captured[5])?;
    assert_eq!(upload_body["scores"][0]["type"], "standard");
    assert_eq!(upload_body["scores"][0]["level_index"], 0);
    assert_eq!(upload_body["scores"][0]["achievements"], 100.1234);
    assert_eq!(upload_body["scores"][1]["type"], "utage");
    assert_eq!(upload_body["scores"][1]["achievements"], 153.5756);
    assert!(captured[5].contains(r#""achievements":100.1234"#));
    assert!(captured[5].contains(r#""achievements":153.5756"#));
    Ok(())
}

#[tokio::test]
async fn score_upload_rejects_mismatched_achievement_before_network() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _client = client(
        Url::parse(&format!("http://{address}/api/v0/"))?,
        Duration::from_secs(1),
        "access-token",
    )?;
    let utage = maimai_core::UtageScore::from_ten_thousandths(1_535_756).into();
    let ranked = AchievementRate::from_decimal_str("100.1234")?.into();
    for (chart_type, achievement) in [
        (LxnsChartType::Standard, utage),
        (LxnsChartType::Deluxe, utage),
        (LxnsChartType::Utage, ranked),
    ] {
        let error = ScoreUpload::new(
            LxnsSongId::new(8)?,
            chart_type,
            LxnsDifficulty::Basic,
            achievement,
            None,
            None,
            1_000,
        )
        .err()
        .ok_or_else(|| io::Error::other("mismatched upload should fail"))?;
        assert_eq!(error.code(), LxnsScoreErrorCode::InvalidRequest);
    }
    assert!(
        timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn errors_are_classified_and_secrets_are_redacted() -> TestResult {
    let cases = [
        (
            MockResponse {
                status: 401,
                body: json!({
                    "authorization":"Bearer SECRET_SENTINEL",
                    "detail":"SECRET_SENTINEL"
                })
                .to_string(),
                delay: Duration::ZERO,
                headers: Vec::new(),
            },
            LxnsScoreErrorCode::Unauthorized,
        ),
        (
            MockResponse {
                status: 500,
                body: json!({"token":"SECRET_SENTINEL"}).to_string(),
                delay: Duration::ZERO,
                headers: Vec::new(),
            },
            LxnsScoreErrorCode::Http,
        ),
        (
            ok(json!({
                "success":false,"message":"bad","access_token":"SECRET_SENTINEL"
            })),
            LxnsScoreErrorCode::Api,
        ),
        (
            MockResponse {
                status: 200,
                body: "not-json SECRET_SENTINEL".to_owned(),
                delay: Duration::ZERO,
                headers: Vec::new(),
            },
            LxnsScoreErrorCode::InvalidJson,
        ),
        (
            ok(json!({"success":true,"data":{"scores":"wrong"}})),
            LxnsScoreErrorCode::InvalidShape,
        ),
    ];

    for (response, expected) in cases {
        let (base_url, _) = mock_server(vec![response]).await?;
        let error = client(base_url, Duration::from_secs(1), "SECRET_SENTINEL")?
            .scores()
            .await
            .err()
            .ok_or_else(|| io::Error::other("expected LXNS score error"))?;
        assert_eq!(error.code(), expected);
        let body = error.body().unwrap_or_default();
        assert!(!body.contains("SECRET_SENTINEL"));
        if expected != LxnsScoreErrorCode::InvalidShape {
            assert!(body.contains("[REDACTED]"));
        }
    }

    let (base_url, _) = mock_server(vec![MockResponse {
        status: 404,
        body: json!({"message":"player not found"}).to_string(),
        delay: Duration::ZERO,
        headers: Vec::new(),
    }])
    .await?;
    let missing = client(base_url, Duration::from_secs(1), "token")?
        .player()
        .await
        .err()
        .ok_or_else(|| io::Error::other("expected missing player error"))?;
    assert!(missing.is_player_not_found());
    Ok(())
}

#[tokio::test]
async fn timeout_and_redirect_are_not_retried_or_followed() -> TestResult {
    let (timeout_url, _) = mock_server(vec![MockResponse {
        status: 200,
        body: json!({"success":true,"data":{}}).to_string(),
        delay: Duration::from_millis(250),
        headers: Vec::new(),
    }])
    .await?;
    let timeout_error = client(timeout_url, Duration::from_millis(100), "token")?
        .player()
        .await
        .err()
        .ok_or_else(|| io::Error::other("expected timeout"))?;
    assert_eq!(timeout_error.code(), LxnsScoreErrorCode::Timeout);

    let (redirect_url, mut requests) = mock_server(vec![MockResponse {
        status: 302,
        body: json!({"message":"redirect"}).to_string(),
        delay: Duration::ZERO,
        headers: vec![("Location".to_owned(), "/redirected".to_owned())],
    }])
    .await?;
    let redirect_error = client(redirect_url, Duration::from_secs(1), "token")?
        .player()
        .await
        .err()
        .ok_or_else(|| io::Error::other("expected redirect error"))?;
    assert_eq!(redirect_error.code(), LxnsScoreErrorCode::Redirect);
    assert_eq!(drain_requests(&mut requests).await.len(), 1);
    Ok(())
}

#[tokio::test]
async fn declared_and_chunked_oversized_responses_are_rejected() -> TestResult {
    for chunked in [false, true] {
        let base_url = oversized_server(chunked).await?;
        let error = client(base_url, Duration::from_secs(2), "token")?
            .player()
            .await
            .err()
            .ok_or_else(|| io::Error::other("oversized response accepted"))?;
        assert_eq!(error.code(), LxnsScoreErrorCode::ResponseTooLarge);
        assert_eq!(error.status(), Some(200));
        assert!(error.body().is_none());
    }
    Ok(())
}

#[tokio::test]
async fn endpoint_authorization_reuses_one_http_connection_across_rotated_tokens() -> TestResult {
    let (base_url, mut requests) = keep_alive_server().await?;
    let endpoint = LxnsScoreEndpoint::new(base_url, Duration::from_secs(1))?;
    endpoint
        .authorize(SecretString::from("first-token"))?
        .player()
        .await?;
    endpoint
        .authorize(SecretString::from("rotated-token"))?
        .player()
        .await?;
    let captured = drain_requests(&mut requests).await;
    assert_eq!(captured.len(), 2);
    assert!(
        captured[0]
            .to_ascii_lowercase()
            .contains("bearer first-token")
    );
    assert!(
        captured[1]
            .to_ascii_lowercase()
            .contains("bearer rotated-token")
    );
    Ok(())
}

#[test]
fn configuration_rejects_secret_bearing_urls_without_echo() -> TestResult {
    let url = Url::parse("https://user:pass@example.test/api/v0")?;
    let error = LxnsScoreConfig::new(
        url,
        Duration::from_secs(1),
        SecretString::from("SECRET_SENTINEL".to_owned()),
    )
    .err()
    .ok_or_else(|| io::Error::other("expected invalid config"))?;
    assert_eq!(error.code(), LxnsScoreErrorCode::InvalidConfiguration);
    assert!(!error.to_string().contains("SECRET_SENTINEL"));
    Ok(())
}

#[test]
fn configuration_enforces_timeout_boundaries() -> TestResult {
    let base_url = Url::parse("https://maimai.lxns.net/api/v0/")?;
    for invalid in [
        Duration::from_millis(99),
        Duration::from_secs(300) + Duration::from_millis(1),
    ] {
        let error = LxnsScoreConfig::new(base_url.clone(), invalid, SecretString::from("token"))
            .err()
            .ok_or_else(|| io::Error::other("invalid timeout accepted"))?;
        assert_eq!(error.code(), LxnsScoreErrorCode::InvalidConfiguration);
    }
    for valid in [Duration::from_millis(100), Duration::from_secs(300)] {
        LxnsScoreConfig::new(base_url.clone(), valid, SecretString::from("token"))?;
    }
    Ok(())
}

#[test]
fn error_redaction_handles_json_escaped_access_token() {
    let token = "escaped\\token";
    let body = json!({"message": format!("echo {token}")}).to_string();
    let sanitized = super::redaction::sanitize_error_body(&body, token);
    assert!(!sanitized.contains(token));
    assert!(!sanitized.contains("escaped\\\\token"));
    assert!(sanitized.contains("[REDACTED]"));
}

fn client(
    base_url: Url,
    timeout: Duration,
    token: &str,
) -> Result<LxnsScoreClient, super::LxnsScoreError> {
    LxnsScoreClient::new(LxnsScoreConfig::new(
        base_url,
        timeout,
        SecretString::from(token.to_owned()),
    )?)
}

fn ok(body: Value) -> MockResponse {
    MockResponse {
        status: 200,
        body: body.to_string(),
        delay: Duration::ZERO,
        headers: Vec::new(),
    }
}

async fn mock_server(
    responses: Vec<MockResponse>,
) -> Result<(Url, mpsc::Receiver<String>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(responses.len().max(1));
    tokio::spawn(async move {
        for response in responses {
            if serve_once(&listener, response, &sender).await.is_err() {
                break;
            }
        }
    });
    Ok((Url::parse(&format!("http://{address}/api/v0/"))?, receiver))
}

async fn oversized_server(chunked: bool) -> Result<Url, Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        let result = async {
            let (mut stream, _) = listener.accept().await?;
            let _ = read_request_from_stream(&mut stream).await?;
            if !chunked {
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            MAX_RESPONSE_BODY_BYTES + 1
                        )
                        .as_bytes(),
                    )
                    .await?;
                return stream.shutdown().await;
            }

            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await?;
            let chunk = vec![b'x'; 64 * 1024];
            let header = format!("{:x}\r\n", chunk.len());
            for _ in 0..=(MAX_RESPONSE_BODY_BYTES / chunk.len()) {
                if stream.write_all(header.as_bytes()).await.is_err()
                    || stream.write_all(&chunk).await.is_err()
                    || stream.write_all(b"\r\n").await.is_err()
                {
                    return Ok(());
                }
            }
            let _ = stream.write_all(b"0\r\n\r\n").await;
            Ok(())
        }
        .await;
        if result.is_err() {
            // 客户端会通过错误码断言读取边界，不输出响应内容。
        }
    });
    Ok(Url::parse(&format!("http://{address}/api/v0/"))?)
}

async fn keep_alive_server() -> Result<(Url, mpsc::Receiver<String>), Box<dyn Error + Send + Sync>>
{
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(2);
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        for index in 0..2 {
            let Ok(request) = read_request_from_stream(&mut stream).await else {
                return;
            };
            if sender.send(request).await.is_err() {
                return;
            }
            let body = json!({"success":true,"data":{"name":"tester"}}).to_string();
            let connection = if index == 0 { "keep-alive" } else { "close" };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n{}",
                body.len(),
                connection,
                body
            );
            if stream.write_all(response.as_bytes()).await.is_err() {
                return;
            }
        }
    });
    Ok((Url::parse(&format!("http://{address}/api/v0/"))?, receiver))
}

async fn read_request_from_stream(stream: &mut tokio::net::TcpStream) -> Result<String, io::Error> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended before headers",
            ));
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(String::from_utf8_lossy(&request).into_owned());
        }
    }
}

async fn serve_once(
    listener: &TcpListener,
    response: MockResponse,
    sender: &mpsc::Sender<String>,
) -> Result<(), io::Error> {
    let (mut stream, _) = listener.accept().await?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended before headers",
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
    let _ = sender
        .send(String::from_utf8_lossy(&request).into_owned())
        .await;
    sleep(response.delay).await;
    let reason = if response.status >= 400 {
        "Error"
    } else {
        "OK"
    };
    let extra_headers = response
        .headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    let encoded = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        extra_headers,
        response.body.len(),
        response.body
    );
    stream.write_all(encoded.as_bytes()).await?;
    stream.shutdown().await
}

async fn drain_requests(receiver: &mut mpsc::Receiver<String>) -> Vec<String> {
    let mut requests = Vec::new();
    while let Some(request) = receiver.recv().await {
        requests.push(request);
    }
    requests
}

fn request_body(request: &str) -> Result<Value, io::Error> {
    let (_, body) = request
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::other("request body separator missing"))?;
    serde_json::from_str(body).map_err(io::Error::other)
}
