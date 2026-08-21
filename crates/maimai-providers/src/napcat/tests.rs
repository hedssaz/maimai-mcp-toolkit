use std::{collections::BTreeMap, io, time::Duration};

use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
    time::sleep,
};
use url::Url;

use super::{MAX_RESPONSE_BODY_BYTES, NapCatClient, NapCatConfig, NapCatErrorCode};

struct MockResponse {
    status: u16,
    body: String,
    delay: Duration,
}

async fn mock_server(
    responses: Vec<MockResponse>,
) -> Result<(Url, mpsc::Receiver<String>), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let capacity = responses.len().max(1);
    let (request_tx, request_rx) = mpsc::channel(capacity);
    tokio::spawn(async move {
        for response in responses {
            if serve_once(&listener, response, &request_tx).await.is_err() {
                break;
            }
        }
    });
    Ok((
        Url::parse(&format!("http://{address}/onebot/"))?,
        request_rx,
    ))
}

enum OversizedFraming {
    ContentLength,
    Chunked(Vec<u8>),
}

async fn oversized_server(framing: OversizedFraming) -> Result<Url, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).await;
        match framing {
            OversizedFraming::ContentLength => {
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    MAX_RESPONSE_BODY_BYTES + 1
                );
                let _ = stream.write_all(headers.as_bytes()).await;
            }
            OversizedFraming::Chunked(chunk) => {
                let headers = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                if stream.write_all(headers).await.is_ok() {
                    let prefix = format!("{:X}\r\n", chunk.len());
                    for _ in 0..=(MAX_RESPONSE_BODY_BYTES / chunk.len()) {
                        if stream.write_all(prefix.as_bytes()).await.is_err()
                            || stream.write_all(&chunk).await.is_err()
                            || stream.write_all(b"\r\n").await.is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }
        let _ = stream.shutdown().await;
    });
    Ok(Url::parse(&format!("http://{address}/onebot/"))?)
}

async fn serve_once(
    listener: &TcpListener,
    response: MockResponse,
    request_tx: &mpsc::Sender<String>,
) -> Result<(), io::Error> {
    let (mut stream, _) = listener.accept().await?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "mock request ended before headers",
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
    let _ = request_tx
        .send(String::from_utf8_lossy(&request).into_owned())
        .await;

    sleep(response.delay).await;
    let reason = if response.status >= 400 {
        "Error"
    } else {
        "OK"
    };
    let encoded = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        response.body.len(),
        response.body
    );
    stream.write_all(encoded.as_bytes()).await?;
    stream.shutdown().await
}

fn response(body: Value) -> MockResponse {
    MockResponse {
        status: 200,
        body: body.to_string(),
        delay: Duration::ZERO,
    }
}

fn client(
    base_url: Url,
    timeout: Duration,
    access_token: &str,
) -> Result<NapCatClient, Box<dyn std::error::Error>> {
    let config = NapCatConfig::new(
        base_url,
        timeout,
        Some(SecretString::from(access_token.to_owned())),
    )?;
    Ok(NapCatClient::new(config)?)
}

fn request_body(request: &str) -> Result<Value, serde_json::Error> {
    let body = request.split_once("\r\n\r\n").map_or("", |parts| parts.1);
    serde_json::from_str(body)
}

fn query_map(url: &Url) -> BTreeMap<String, String> {
    url.query_pairs().into_owned().collect()
}

#[test]
fn configuration_rejects_ambiguous_urls_and_tokens() -> Result<(), Box<dyn std::error::Error>> {
    let invalid_url = Url::parse("http://user:password@napcat:3000/?token=leak")?;
    assert!(NapCatConfig::new(invalid_url, Duration::from_secs(1), None).is_err());

    let valid_url = Url::parse("http://napcat:3000/")?;
    for token in ["", "   ", " leading", "two words", "trailing "] {
        assert!(
            NapCatConfig::new(
                valid_url.clone(),
                Duration::from_secs(1),
                Some(SecretString::from(token.to_owned())),
            )
            .is_err(),
            "token should be rejected: {token:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn one_client_queries_all_three_endpoints_with_typed_models_and_auth()
-> Result<(), Box<dyn std::error::Error>> {
    let access_token = "napcat-access-token-sentinel";
    let responses = vec![
        response(json!({
            "status": "ok",
            "retcode": 0,
            "data": [{"user_id": 10001, "nickname": " Alice ", "remark": "discard"}],
            "message": "",
            "wording": ""
        })),
        response(json!([
            {"groupId": "20001", "groupName": " Mai Group ", "memberCount": 2}
        ])),
        response(json!({
            "status": "ok",
            "retcode": 0,
            "data": [
                {"userId": "10001", "nickname": "Alice", "card": " Captain "},
                {"group_id": 20001, "user_id": 10002, "nickname": "Bob", "card": ""}
            ]
        })),
    ];
    let (base_url, mut captured) = mock_server(responses).await?;
    assert!(query_map(&base_url).is_empty());
    let client = client(base_url, Duration::from_secs(2), access_token)?;
    let debug = format!("{client:?}");
    assert!(!debug.contains(access_token));
    assert!(!debug.to_ascii_lowercase().contains("authorization: bearer"));

    let friends = client.get_friend_list().await?;
    assert_eq!(friends.status(), Some("ok"));
    assert_eq!(friends.retcode(), Some(0));
    assert_eq!(friends.data()[0].user_id(), "10001");
    assert_eq!(friends.data()[0].nickname(), Some("Alice"));

    let groups = client.get_group_list().await?;
    assert_eq!(groups.status(), None);
    assert_eq!(groups.data()[0].group_id(), "20001");
    assert_eq!(groups.data()[0].group_name(), Some("Mai Group"));
    assert_eq!(groups.data()[0].member_count(), Some(2));

    let members = client.get_group_member_list("20001", true).await?;
    assert_eq!(members.data()[0].group_id(), "20001");
    assert_eq!(members.data()[0].card(), Some("Captain"));
    assert_eq!(members.data()[1].group_id(), "20001");
    assert_eq!(members.data()[1].card(), None);

    let friend_request = captured.recv().await.ok_or("missing friend request")?;
    let group_request = captured.recv().await.ok_or("missing group request")?;
    let member_request = captured.recv().await.ok_or("missing member request")?;
    for (request, path) in [
        (&friend_request, "/onebot/get_friend_list"),
        (&group_request, "/onebot/get_group_list"),
        (&member_request, "/onebot/get_group_member_list"),
    ] {
        let lowered = request.to_ascii_lowercase();
        assert!(lowered.starts_with(&format!("post {path} http/1.1")));
        assert!(lowered.contains("authorization: bearer napcat-access-token-sentinel"));
    }
    assert_eq!(request_body(&friend_request)?, json!({}));
    assert_eq!(request_body(&group_request)?, json!({}));
    assert_eq!(
        request_body(&member_request)?,
        json!({"group_id": 20001, "no_cache": true})
    );
    Ok(())
}

#[tokio::test]
async fn retcode_error_preserves_structured_fields_without_secret_or_header_leaks()
-> Result<(), Box<dyn std::error::Error>> {
    let access_token = "arbitrary-token-sentinel";
    let body = json!({
        "status": "failed",
        "retcode": 1404,
        "message": format!("Authorization: Bearer {access_token}"),
        "wording": "fallback",
        "data": null,
        "access_token": access_token
    });
    let (base_url, _) = mock_server(vec![response(body)]).await?;
    let client = client(base_url, Duration::from_secs(2), access_token)?;

    let error = client
        .get_friend_list()
        .await
        .err()
        .ok_or("expected OneBot error")?;
    assert_eq!(error.code(), NapCatErrorCode::OneBot);
    assert_eq!(error.onebot_status(), Some("failed"));
    assert_eq!(error.retcode(), Some(1404));
    for rendered in [
        error.to_string(),
        format!("{error:?}"),
        error.provider_message().unwrap_or_default().to_owned(),
        error.body().unwrap_or_default().to_owned(),
    ] {
        assert!(!rendered.contains(access_token));
        assert!(!rendered.contains(&format!("Bearer {access_token}")));
    }
    Ok(())
}

#[tokio::test]
async fn malformed_json_shape_and_items_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let responses = vec![
        MockResponse {
            status: 200,
            body: "not-json".to_owned(),
            delay: Duration::ZERO,
        },
        response(json!({"status": "ok", "retcode": 0})),
        response(json!({
            "status": "ok",
            "retcode": 0,
            "data": [{"user_id": "not-a-qq", "nickname": "bad"}]
        })),
    ];
    let (base_url, _) = mock_server(responses).await?;
    let client = client(base_url, Duration::from_secs(2), "secret")?;
    for _ in 0..3 {
        let error = client
            .get_friend_list()
            .await
            .err()
            .ok_or("expected invalid response")?;
        assert_eq!(error.code(), NapCatErrorCode::InvalidResponse);
    }
    Ok(())
}

#[tokio::test]
async fn request_timeout_is_structured_and_does_not_leak_token()
-> Result<(), Box<dyn std::error::Error>> {
    let access_token = "timeout-token-sentinel";
    let (base_url, _) = mock_server(vec![MockResponse {
        status: 200,
        body: json!({"status": "ok", "retcode": 0, "data": []}).to_string(),
        delay: Duration::from_millis(200),
    }])
    .await?;
    let client = client(base_url, Duration::from_millis(30), access_token)?;

    let error = client
        .get_group_list()
        .await
        .err()
        .ok_or("expected timeout")?;
    assert_eq!(error.code(), NapCatErrorCode::Timeout);
    assert!(!error.to_string().contains(access_token));
    assert!(!format!("{error:?}").contains(access_token));
    Ok(())
}

#[tokio::test]
async fn oversized_content_length_is_rejected_before_reading_the_body()
-> Result<(), Box<dyn std::error::Error>> {
    let access_token = "content-length-token-sentinel";
    let base_url = oversized_server(OversizedFraming::ContentLength).await?;
    let client = client(base_url, Duration::from_secs(2), access_token)?;

    let error = client
        .get_friend_list()
        .await
        .err()
        .ok_or("expected oversized response")?;

    assert_eq!(error.code(), NapCatErrorCode::InvalidResponse);
    assert!(error.to_string().contains("超过大小限制"));
    assert!(error.body().is_none());
    assert!(!error.to_string().contains(access_token));
    assert!(!format!("{error:?}").contains(access_token));
    Ok(())
}

#[tokio::test]
async fn oversized_chunked_body_is_bounded_and_does_not_leak_token()
-> Result<(), Box<dyn std::error::Error>> {
    let access_token = "chunked-token-sentinel";
    let mut chunk = vec![b'x'; 256 * 1024];
    chunk[..access_token.len()].copy_from_slice(access_token.as_bytes());
    let base_url = oversized_server(OversizedFraming::Chunked(chunk)).await?;
    let client = client(base_url, Duration::from_secs(5), access_token)?;

    let error = client
        .get_friend_list()
        .await
        .err()
        .ok_or("expected oversized response")?;

    assert_eq!(error.code(), NapCatErrorCode::InvalidResponse);
    assert!(error.to_string().contains("超过大小限制"));
    assert!(error.body().is_none());
    assert!(!error.to_string().contains(access_token));
    assert!(!format!("{error:?}").contains(access_token));
    Ok(())
}
