use std::{collections::BTreeMap, io, time::Duration};

use secrecy::ExposeSecret;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    time::timeout,
};

use super::{
    AuthRequirement, DivingFishClient, DivingFishCredentials, DivingFishGame, DivingFishOperation,
    DivingFishRequest, HttpMethod, MAX_RESPONSE_BODY_BYTES, ProviderErrorCode, QueryValue,
};

struct MockResponse {
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    body: String,
}

async fn mock_server(
    response: MockResponse,
) -> Result<(String, oneshot::Receiver<String>), io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (request_tx, request_rx) = oneshot::channel();
    tokio::spawn(async move {
        let result = async {
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
            let request_text = String::from_utf8_lossy(&request).into_owned();
            let _ = request_tx.send(request_text);

            let reason = if response.status >= 400 {
                "Error"
            } else {
                "OK"
            };
            let mut encoded = format!(
                "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                response.status,
                reason,
                response.body.len()
            );
            for (name, value) in response.headers {
                encoded.push_str(name);
                encoded.push_str(": ");
                encoded.push_str(value);
                encoded.push_str("\r\n");
            }
            encoded.push_str("\r\n");
            encoded.push_str(&response.body);
            stream.write_all(encoded.as_bytes()).await?;
            stream.shutdown().await
        }
        .await;
        if result.is_err() {
            // 测试断言会因接收端关闭而失败，无需向 stdout/stderr 输出请求内容。
        }
    });
    Ok((format!("http://{address}/api/"), request_rx))
}

async fn oversized_response_server(chunked: bool) -> Result<String, io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        let result = async {
            let (mut stream, _) = listener.accept().await?;
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await?;
            if !chunked {
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            MAX_RESPONSE_BODY_BYTES + 1
                        )
                        .as_bytes(),
                    )
                    .await?;
                return stream.shutdown().await;
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
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
            stream.shutdown().await
        }
        .await;
        if result.is_err() {
            // 调用端会断言失败，无需输出测试请求。
        }
    });
    Ok(format!("http://{address}/api/"))
}

async fn read_headers(stream: &mut tokio::net::TcpStream) -> Result<String, io::Error> {
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
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            return Ok(String::from_utf8_lossy(&request[..index + 4]).into_owned());
        }
    }
}

fn cover_base(api_base: &str) -> String {
    api_base.replace("/api/", "/covers/")
}

#[test]
fn catalog_has_all_33_legacy_operations() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(DivingFishOperation::ALL.len(), 33);
    for operation in DivingFishOperation::ALL {
        assert_eq!(
            operation.to_string().parse::<DivingFishOperation>()?,
            operation
        );
    }
    let update = DivingFishOperation::MaimaiPlayerUpdateRecordsPost.metadata();
    assert_eq!(update.game, DivingFishGame::MaimaiDxProber);
    assert_eq!(update.method, HttpMethod::Post);
    assert_eq!(update.path, "/player/update_records");
    assert_eq!(update.auth, AuthRequirement::LoginOrImportToken);
    assert!(update.mutating);
    assert!(!update.destructive);
    assert!(!DivingFishOperation::MaimaiCoverUrl.metadata().http);
    Ok(())
}

#[tokio::test]
async fn sends_auth_etag_query_and_json_without_debug_secret_leaks()
-> Result<(), Box<dyn std::error::Error>> {
    let developer_token = "developer-secret-sentinel";
    let (api_base, captured) = mock_server(MockResponse {
        status: 200,
        headers: vec![("Content-Type", "application/json")],
        body: r#"{"records":[]}"#.to_owned(),
    })
    .await?;
    let client = DivingFishClient::with_base_urls(&api_base, &cover_base(&api_base))?;
    let credentials = DivingFishCredentials::new().with_developer_token(developer_token);
    let extra_headers = BTreeMap::from([("x-trace-id".to_owned(), "trace-1".to_owned())]);
    let request = DivingFishRequest::new(DivingFishOperation::MaimaiDevPlayerRecordPost)
        .with_query("qq", "123456")
        .with_query(
            "music_id",
            QueryValue::Multiple(vec!["1".to_owned(), "2".to_owned()]),
        )
        .with_body(json!({"qq": "123456", "music_id": [1, 2]}))
        .try_with_headers(extra_headers)?
        .try_with_if_none_match("\"etag-1\"")?
        .with_timeout(Duration::from_secs(2))
        .with_credentials(credentials);
    let debug = format!("{request:?}");
    assert!(!debug.contains(developer_token));
    assert!(!debug.contains("123456"));

    let response = client.execute(request).await?;
    let request_text = captured.await?;
    let lowered = request_text.to_ascii_lowercase();
    assert!(lowered.starts_with("post /api/maimaidxprober/dev/player/record?"));
    assert!(lowered.contains("qq=123456"));
    assert!(lowered.contains("music_id=1"));
    assert!(lowered.contains("music_id=2"));
    assert!(lowered.contains("developer-token: developer-secret-sentinel"));
    assert!(lowered.contains("if-none-match: \"etag-1\""));
    assert!(lowered.contains("x-trace-id: trace-1"));
    let request_body = request_text
        .split_once("\r\n\r\n")
        .map_or("", |parts| parts.1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(request_body)?,
        json!({"qq": "123456", "music_id": [1, 2]})
    );
    assert_eq!(response.status(), 200);
    assert!(!format!("{response:?}").contains(developer_token));
    assert!(!format!("{response:?}").contains("123456"));
    Ok(())
}

#[tokio::test]
async fn mutating_operations_require_exact_typed_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let client =
        DivingFishClient::with_base_urls("http://127.0.0.1:9/api/", "http://127.0.0.1:9/covers/")?;
    let credentials = DivingFishCredentials::new().with_import_token("import-secret");
    let request = DivingFishRequest::new(DivingFishOperation::MaimaiPlayerUpdateRecordsPost)
        .with_body(json!([]))
        .with_credentials(credentials);
    let error = client
        .execute(request)
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(error.code(), ProviderErrorCode::ConfirmationRequired);

    let credentials = DivingFishCredentials::new().with_import_token("import-secret");
    let request = DivingFishRequest::new(DivingFishOperation::MaimaiPlayerUpdateRecordsPost)
        .with_body(json!([]))
        .confirm(DivingFishOperation::PublicMessagePost)
        .with_credentials(credentials);
    let error = client
        .execute(request)
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(error.code(), ProviderErrorCode::ConfirmationRequired);
    Ok(())
}

#[tokio::test]
async fn rejects_untyped_credential_headers() -> Result<(), Box<dyn std::error::Error>> {
    let secret = "header-secret-sentinel";
    let headers = BTreeMap::from([("authorization".to_owned(), secret.to_owned())]);
    let error = DivingFishRequest::new(DivingFishOperation::PublicAliveCheckGet)
        .try_with_headers(headers)
        .err()
        .ok_or("expected error")?;
    assert_eq!(error.code(), ProviderErrorCode::InvalidRequest);
    assert!(!format!("{error:?}").contains(secret));
    Ok(())
}

#[tokio::test]
async fn raw_body_and_successful_confirmation_reach_the_server()
-> Result<(), Box<dyn std::error::Error>> {
    let (api_base, captured) = mock_server(MockResponse {
        status: 200,
        headers: vec![],
        body: "ok".to_owned(),
    })
    .await?;
    let client = DivingFishClient::with_base_urls(&api_base, &cover_base(&api_base))?;
    let request = DivingFishRequest::new(DivingFishOperation::ChunithmUpdateRecordsHtmlPost)
        .with_raw_body("<html>fixture</html>")
        .confirm(DivingFishOperation::ChunithmUpdateRecordsHtmlPost)
        .with_credentials(DivingFishCredentials::new().with_import_token("import-secret"));
    let response = client.execute(request).await?;
    let request_text = captured.await?;
    let lowered = request_text.to_ascii_lowercase();
    assert!(lowered.contains("content-type: text/html; charset=utf-8"));
    assert!(lowered.contains("import-token: import-secret"));
    assert!(request_text.ends_with("<html>fixture</html>"));
    assert_eq!(response.text(), Some("ok"));
    Ok(())
}

#[tokio::test]
async fn extracts_login_cookie_as_secret() -> Result<(), Box<dyn std::error::Error>> {
    let jwt = "jwt-secret-sentinel";
    let (api_base, captured) = mock_server(MockResponse {
        status: 200,
        headers: vec![(
            "Set-Cookie",
            "jwt_token=jwt-secret-sentinel; Path=/; HttpOnly",
        )],
        body: "{}".to_owned(),
    })
    .await?;
    let client = DivingFishClient::with_base_urls(&api_base, &cover_base(&api_base))?;
    let request = DivingFishRequest::new(DivingFishOperation::MaimaiLogin)
        .with_credentials(DivingFishCredentials::new().with_login("user", "password-secret"));
    assert!(!format!("{request:?}").contains("password-secret"));
    let response = client.execute(request).await?;
    let request_text = captured.await?;
    assert!(request_text.contains(r#"{"username":"user","password":"password-secret"}"#));
    assert_eq!(
        response.jwt_token().map(ExposeSecret::expose_secret),
        Some(jwt)
    );
    assert!(!response.headers().contains_key("set-cookie"));
    assert!(!format!("{response:?}").contains(jwt));
    Ok(())
}

#[tokio::test]
async fn http_errors_preserve_status_and_sanitize_body() -> Result<(), Box<dyn std::error::Error>> {
    let token = "developer-secret-sentinel";
    let (api_base, _captured) = mock_server(MockResponse {
        status: 401,
        headers: vec![("Content-Type", "application/json")],
        body: format!(r#"{{"message":"rejected {token}","access_token":"server-secret"}}"#),
    })
    .await?;
    let client = DivingFishClient::with_base_urls(&api_base, &cover_base(&api_base))?;
    let request = DivingFishRequest::new(DivingFishOperation::MaimaiDevPlayerRecordsGet)
        .with_credentials(DivingFishCredentials::new().with_developer_token(token));
    let error = client
        .execute(request)
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(error.code(), ProviderErrorCode::Http);
    assert_eq!(error.status(), Some(401));
    let body = error.body().ok_or("sanitized body missing")?;
    assert!(!body.contains(token));
    assert!(!body.contains("server-secret"));
    assert!(body.contains("[REDACTED]"));
    assert!(!error.to_string().contains(token));
    assert!(!format!("{error:?}").contains(token));
    Ok(())
}

#[tokio::test]
async fn redirects_are_not_followed_and_expose_only_status()
-> Result<(), Box<dyn std::error::Error>> {
    let secret = "redirect-secret-sentinel";
    let (api_base, captured) = mock_server(MockResponse {
        status: 302,
        headers: vec![("Location", "http://127.0.0.1:9/redirect-secret-sentinel")],
        body: format!(r#"{{"token":"{secret}"}}"#),
    })
    .await?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let client = DivingFishClient::with_http_client(http, &api_base, &cover_base(&api_base))?;
    let error = client
        .execute(DivingFishRequest::new(
            DivingFishOperation::PublicAliveCheckGet,
        ))
        .await
        .err()
        .ok_or("redirect should fail")?;
    assert_eq!(error.code(), ProviderErrorCode::Redirect);
    assert_eq!(error.status(), Some(302));
    assert!(error.body().is_none());
    assert!(!format!("{error:?}").contains(secret));
    assert!(
        captured
            .await?
            .starts_with("GET /api/maimaidxprober/alive_check HTTP/1.1")
    );
    Ok(())
}

#[tokio::test]
async fn default_client_never_forwards_import_token_across_origin_redirect()
-> Result<(), Box<dyn std::error::Error>> {
    let redirected = TcpListener::bind("127.0.0.1:0").await?;
    let redirected_address = redirected.local_addr()?;
    let source = TcpListener::bind("127.0.0.1:0").await?;
    let source_address = source.local_addr()?;
    let (captured_tx, captured_rx) = oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let (mut stream, _) = source.accept().await?;
            let request = read_headers(&mut stream).await?;
            let _ = captured_tx.send(request);
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{redirected_address}/sink\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await
        }
        .await;
        if result.is_err() {
            // 调用端断开会由测试断言暴露；不输出可能含 token 的请求。
        }
    });

    let api_base = format!("http://{source_address}/api/");
    let client = DivingFishClient::with_base_urls(&api_base, &cover_base(&api_base))?;
    let operation = DivingFishOperation::MaimaiPlayerUpdateRecordsPost;
    let error = client
        .execute(
            DivingFishRequest::new(operation)
                .with_body(json!([]))
                .confirm(operation)
                .with_credentials(
                    DivingFishCredentials::new().with_import_token("redirect-import-secret"),
                ),
        )
        .await
        .err()
        .ok_or("redirect should fail")?;
    assert_eq!(error.code(), ProviderErrorCode::Redirect);
    let captured = captured_rx.await?.to_ascii_lowercase();
    assert!(captured.contains("import-token: redirect-import-secret"));
    assert!(
        timeout(Duration::from_millis(100), redirected.accept())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn injected_http_client_keeps_its_redirect_policy() -> Result<(), Box<dyn std::error::Error>>
{
    let redirected = TcpListener::bind("127.0.0.1:0").await?;
    let redirected_address = redirected.local_addr()?;
    let source = TcpListener::bind("127.0.0.1:0").await?;
    let source_address = source.local_addr()?;
    tokio::spawn(async move {
        let result = async {
            let (mut stream, _) = source.accept().await?;
            let _ = read_headers(&mut stream).await?;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://{redirected_address}/sink\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await?;
            stream.shutdown().await
        }
        .await;
        if result.is_err() {
            // 调用端断开会由断言暴露；请求不含凭据。
        }
    });
    let redirected_task = tokio::spawn(async move {
        let (mut stream, _) = redirected.accept().await?;
        let request = read_headers(&mut stream).await?;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            )
            .await?;
        stream.shutdown().await?;
        Ok::<_, io::Error>(request)
    });

    let http = reqwest::Client::builder().build()?;
    let api_base = format!("http://{source_address}/api/");
    let client = DivingFishClient::with_http_client(http, &api_base, &cover_base(&api_base))?;
    let response = client
        .execute(DivingFishRequest::new(
            DivingFishOperation::PublicAliveCheckGet,
        ))
        .await?;
    assert_eq!(response.status(), 200);
    assert!(redirected_task.await??.starts_with("GET /sink HTTP/1.1"));
    Ok(())
}

#[tokio::test]
async fn request_timeout_accepts_only_point_one_through_three_hundred_seconds()
-> Result<(), Box<dyn std::error::Error>> {
    let unreachable =
        DivingFishClient::with_base_urls("http://127.0.0.1:9/api/", "http://127.0.0.1:9/covers/")?;
    for invalid in [
        Duration::from_millis(99),
        Duration::from_secs(300) + Duration::from_millis(1),
    ] {
        let error = unreachable
            .execute(
                DivingFishRequest::new(DivingFishOperation::PublicAliveCheckGet)
                    .with_timeout(invalid),
            )
            .await
            .err()
            .ok_or("invalid timeout reached the network")?;
        assert_eq!(error.code(), ProviderErrorCode::InvalidRequest);
    }

    for valid in [Duration::from_millis(100), Duration::from_secs(300)] {
        let (api_base, captured) = mock_server(MockResponse {
            status: 200,
            headers: Vec::new(),
            body: "{}".to_owned(),
        })
        .await?;
        let client = DivingFishClient::with_base_urls(&api_base, &cover_base(&api_base))?;
        client
            .execute(
                DivingFishRequest::new(DivingFishOperation::PublicAliveCheckGet)
                    .with_timeout(valid),
            )
            .await?;
        let _ = captured.await?;
    }
    Ok(())
}

#[tokio::test]
async fn declared_and_chunked_oversized_responses_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    for chunked in [false, true] {
        let api_base = oversized_response_server(chunked).await?;
        let client = DivingFishClient::with_base_urls(&api_base, &cover_base(&api_base))?;
        let error = client
            .execute(DivingFishRequest::new(
                DivingFishOperation::MaimaiRatingRankingGet,
            ))
            .await
            .err()
            .ok_or("oversized response should fail")?;
        assert_eq!(error.code(), ProviderErrorCode::BodyTooLarge);
        assert_eq!(error.status(), Some(200));
        assert!(error.body().is_none());
    }
    Ok(())
}

#[test]
fn error_redaction_handles_json_escaped_credentials() {
    let secret = "escaped\\credential".to_owned();
    let body = json!({"message": format!("echo {secret}")}).to_string();
    let sanitized = super::redaction::sanitize_error_body(&body, std::slice::from_ref(&secret));
    assert!(!sanitized.contains(&secret));
    assert!(!sanitized.contains("escaped\\\\credential"));
    assert!(sanitized.contains("[REDACTED]"));
}

#[test]
fn cover_operation_keeps_legacy_id_mapping() -> Result<(), Box<dyn std::error::Error>> {
    let client = DivingFishClient::new()?;
    assert!(client.cover_url(10_038)?.as_str().ends_with("/00038.png"));
    assert!(client.cover_url(11_001)?.as_str().ends_with("/11001.png"));
    Ok(())
}
