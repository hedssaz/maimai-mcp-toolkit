use std::{collections::BTreeMap, io, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest as _, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};
use url::{Url, form_urlencoded};

use super::{
    DEFAULT_SCOPES, LxnsOAuthClient, MAX_TOKEN_RESPONSE_BYTES, OAuthConfig, OAuthErrorCode,
    OAuthState, PkceVerifier,
};

struct MockResponse {
    status: u16,
    body: String,
}

async fn mock_server(
    response: MockResponse,
) -> Result<(Url, oneshot::Receiver<String>), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (request_tx, request_rx) = oneshot::channel();
    tokio::spawn(async move {
        let result = serve_once(listener, response, request_tx).await;
        if result.is_err() {
            // 接收端断开会由测试断言暴露；这里不输出可能包含 secret 的请求。
        }
    });
    Ok((
        Url::parse(&format!("http://{address}/oauth/token"))?,
        request_rx,
    ))
}

async fn serve_once(
    listener: TcpListener,
    response: MockResponse,
    request_tx: oneshot::Sender<String>,
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
    let request_text = String::from_utf8_lossy(&request).into_owned();
    let _ = request_tx.send(request_text);

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

fn make_config(
    token_url: Url,
    client_secret: &str,
) -> Result<OAuthConfig, Box<dyn std::error::Error>> {
    Ok(OAuthConfig::new(
        "lxns-client",
        Some(SecretString::from(client_secret.to_owned())),
        Some(Url::parse("https://bot.example.test/lxns/callback")?),
        Url::parse("https://maimai.lxns.net/oauth/authorize")?,
        token_url,
        DEFAULT_SCOPES.iter().map(ToString::to_string).collect(),
    )?)
}

fn request_form(request: &str) -> BTreeMap<String, String> {
    let body = request.split_once("\r\n\r\n").map_or("", |parts| parts.1);
    form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect()
}

#[test]
fn authorization_url_uses_192_bit_opaque_state_and_pkce_without_subject()
-> Result<(), Box<dyn std::error::Error>> {
    let subject_sentinel = "subject-123456789-sensitive";
    let config = make_config(
        Url::parse("https://maimai.lxns.net/api/v0/oauth/token")?,
        "secret",
    )?;
    let client = LxnsOAuthClient::new(config)?;

    let request = client.authorization_request()?;
    let state = request.state().secret().expose_secret();
    let verifier = request.code_verifier().secret().expose_secret();
    let query = request
        .url()
        .query_pairs()
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    let state_bytes = URL_SAFE_NO_PAD.decode(state)?;
    let verifier_bytes = URL_SAFE_NO_PAD.decode(verifier)?;

    assert_eq!(state_bytes.len(), 24);
    assert_eq!(verifier_bytes.len(), 32);
    assert_eq!(query.get("state").map(String::as_str), Some(state));
    assert_eq!(
        query.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert_eq!(
        query.get("code_challenge").map(String::as_str),
        Some(
            URL_SAFE_NO_PAD
                .encode(Sha256::digest(verifier.as_bytes()))
                .as_str()
        )
    );
    assert!(!request.url().as_str().contains(subject_sentinel));
    assert!(!state.contains(subject_sentinel));
    let debug = format!("{request:?}");
    assert!(!debug.contains(state));
    assert!(!debug.contains(verifier));
    assert!(!debug.contains(subject_sentinel));
    Ok(())
}

#[test]
fn explicit_signed_state_and_scopes_enter_url_without_debug_echo()
-> Result<(), Box<dyn std::error::Error>> {
    let signed_state = "signed.v1.opaque-state.signature-sentinel";
    let config = make_config(
        Url::parse("https://maimai.lxns.net/api/v0/oauth/token")?,
        "secret",
    )?;
    let client = LxnsOAuthClient::new(config)?;
    let state = OAuthState::from_secret(SecretString::from(signed_state.to_owned()))?;
    let scopes = vec!["read_player".to_owned(), "write_player".to_owned()];

    let request = client.authorization_request_with(Some(state), Some(&scopes))?;
    let query = request
        .url()
        .query_pairs()
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(query.get("state").map(String::as_str), Some(signed_state));
    assert_eq!(
        query.get("scope").map(String::as_str),
        Some("read_player write_player")
    );
    assert_eq!(request.state().secret().expose_secret(), signed_state);
    assert!(!format!("{request:?}").contains(signed_state));

    for invalid in [
        String::new(),
        "has whitespace".to_owned(),
        "has\0control".to_owned(),
        "x".repeat(1_025),
    ] {
        let error = OAuthState::from_secret(SecretString::from(invalid))
            .err()
            .ok_or("expected invalid state")?;
        assert_eq!(error.code(), OAuthErrorCode::InvalidRequest);
    }
    Ok(())
}

#[tokio::test]
async fn authorization_code_exchange_posts_expected_form_and_redacts_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let response = r#"{"success":true,"data":{"access_token":"access-secret","refresh_token":"refresh-secret","token_type":"Bearer","scope":"read_player","expires_in":900}}"#;
    let (token_url, captured) = mock_server(MockResponse {
        status: 200,
        body: response.to_owned(),
    })
    .await?;
    let client_secret = "client-secret-sentinel";
    let client = LxnsOAuthClient::new(make_config(token_url, client_secret)?)?
        .with_timeout(Duration::from_secs(2))?;
    let authorization = client.authorization_request()?;
    let code = SecretString::from("authorization-code-sentinel".to_owned());

    let tokens = client
        .exchange_authorization_code(&code, authorization.code_verifier())
        .await?;
    let captured = captured.await?;
    let form = request_form(&captured);

    assert_eq!(
        form.get("grant_type").map(String::as_str),
        Some("authorization_code")
    );
    assert_eq!(
        form.get("client_id").map(String::as_str),
        Some("lxns-client")
    );
    assert_eq!(
        form.get("client_secret").map(String::as_str),
        Some(client_secret)
    );
    assert_eq!(
        form.get("code").map(String::as_str),
        Some(code.expose_secret())
    );
    assert_eq!(
        form.get("code_verifier").map(String::as_str),
        Some(authorization.code_verifier().secret().expose_secret())
    );
    assert_eq!(tokens.access_token().expose_secret(), "access-secret");
    assert_eq!(tokens.refresh_token().expose_secret(), "refresh-secret");
    assert_eq!(tokens.expires_in(), Some(900));
    let debug = format!("{tokens:?}");
    assert!(!debug.contains("access-secret"));
    assert!(!debug.contains("refresh-secret"));
    Ok(())
}

#[tokio::test]
async fn refresh_requires_a_new_rotating_refresh_token() -> Result<(), Box<dyn std::error::Error>> {
    let (token_url, captured) = mock_server(MockResponse {
        status: 200,
        body: r#"{"accessToken":"next-access","refreshToken":"next-refresh","expiresIn":600}"#
            .to_owned(),
    })
    .await?;
    let client = LxnsOAuthClient::new(make_config(token_url, "client-secret")?)?;
    let previous = SecretString::from("previous-refresh".to_owned());

    let tokens = client.refresh_token(&previous).await?;
    let form = request_form(&captured.await?);
    assert_eq!(
        form.get("grant_type").map(String::as_str),
        Some("refresh_token")
    );
    assert_eq!(
        form.get("refresh_token").map(String::as_str),
        Some("previous-refresh")
    );
    assert_eq!(tokens.refresh_token().expose_secret(), "next-refresh");

    let (token_url, _) = mock_server(MockResponse {
        status: 200,
        body: r#"{"access_token":"next-access","refresh_token":"previous-refresh"}"#.to_owned(),
    })
    .await?;
    let client = LxnsOAuthClient::new(make_config(token_url, "client-secret")?)?;
    let error = client
        .refresh_token(&previous)
        .await
        .err()
        .ok_or("expected rotation error")?;
    assert_eq!(error.code(), OAuthErrorCode::RefreshTokenNotRotated);
    assert!(!format!("{error:?}").contains("previous-refresh"));
    Ok(())
}

#[tokio::test]
async fn invalid_grant_and_unauthorized_errors_are_typed_and_fully_redacted()
-> Result<(), Box<dyn std::error::Error>> {
    let client_secret = "client-secret-sentinel";
    let old_refresh = SecretString::from("old-refresh-sentinel".to_owned());
    let body = format!(
        r#"{{"error":"invalid_grant","error_description":"grant {} rejected for {}","refresh_token":"{}"}}"#,
        old_refresh.expose_secret(),
        client_secret,
        old_refresh.expose_secret()
    );
    let (token_url, _) = mock_server(MockResponse { status: 400, body }).await?;
    let client = LxnsOAuthClient::new(make_config(token_url, client_secret)?)?;
    let error = client
        .refresh_token(&old_refresh)
        .await
        .err()
        .ok_or("expected invalid_grant")?;
    assert_eq!(error.code(), OAuthErrorCode::InvalidGrant);
    assert_eq!(error.status(), Some(400));
    for rendered in [
        error.to_string(),
        format!("{error:?}"),
        error.body().unwrap_or_default().to_owned(),
    ] {
        assert!(!rendered.contains(client_secret));
        assert!(!rendered.contains(old_refresh.expose_secret()));
    }

    let code = SecretString::from("authorization-code-sentinel".to_owned());
    let verifier = PkceVerifier::from_secret(SecretString::from(
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~".to_owned(),
    ))?;
    let body = format!(
        r#"{{"error":"unauthorized","error_description":"code {} verifier {} secret {}"}}"#,
        code.expose_secret(),
        verifier.secret().expose_secret(),
        client_secret
    );
    let (token_url, _) = mock_server(MockResponse { status: 401, body }).await?;
    let client = LxnsOAuthClient::new(make_config(token_url, client_secret)?)?;
    let error = client
        .exchange_authorization_code(&code, &verifier)
        .await
        .err()
        .ok_or("expected unauthorized")?;
    assert_eq!(error.code(), OAuthErrorCode::Unauthorized);
    for rendered in [
        error.to_string(),
        format!("{error:?}"),
        error.body().unwrap_or_default().to_owned(),
    ] {
        assert!(!rendered.contains(code.expose_secret()));
        assert!(!rendered.contains(verifier.secret().expose_secret()));
        assert!(!rendered.contains(client_secret));
    }
    Ok(())
}

#[tokio::test]
async fn declared_and_chunked_oversized_token_responses_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    for chunked in [false, true] {
        let token_url = oversized_server(chunked).await?;
        let client = LxnsOAuthClient::new(make_config(token_url, "client-secret")?)?;
        let error = client
            .refresh_token(&SecretString::from("refresh-token"))
            .await
            .err()
            .ok_or("oversized token response accepted")?;
        assert_eq!(error.code(), OAuthErrorCode::InvalidTokenResponse);
        assert_eq!(error.status(), Some(200));
        assert!(error.body().is_none());
    }
    Ok(())
}

#[test]
fn http_timeout_accepts_only_point_one_through_three_hundred_seconds()
-> Result<(), Box<dyn std::error::Error>> {
    let token_url = Url::parse("https://maimai.lxns.net/api/v0/oauth/token")?;
    for invalid in [
        Duration::from_millis(99),
        Duration::from_secs(300) + Duration::from_millis(1),
    ] {
        let error = LxnsOAuthClient::new(make_config(token_url.clone(), "client-secret")?)?
            .with_timeout(invalid)
            .err()
            .ok_or("invalid OAuth timeout accepted")?;
        assert_eq!(error.code(), OAuthErrorCode::InvalidConfiguration);
    }
    for valid in [Duration::from_millis(100), Duration::from_secs(300)] {
        LxnsOAuthClient::new(make_config(token_url.clone(), "client-secret")?)?
            .with_timeout(valid)?;
    }
    Ok(())
}

#[test]
fn config_debug_redacts_secret_and_endpoint_queries() -> Result<(), Box<dyn std::error::Error>> {
    let secret = "client-secret-sentinel";
    let config = OAuthConfig::new(
        "client-id",
        Some(SecretString::from(secret.to_owned())),
        None,
        Url::parse("https://example.test/oauth/authorize?state=state-secret")?,
        Url::parse("https://example.test/oauth/token?credential=query-secret")?,
        vec!["read_player".to_owned()],
    )?;
    let debug = format!("{config:?}");
    assert!(!debug.contains(secret));
    assert!(!debug.contains("state-secret"));
    assert!(!debug.contains("query-secret"));
    assert!(debug.contains("has_client_secret: true"));
    Ok(())
}

#[test]
fn error_redaction_handles_json_escaped_credentials() {
    let secret = "escaped\\credential";
    let body = serde_json::json!({"message": format!("echo {secret}")}).to_string();
    let sanitized = super::redaction::sanitize_error_body(&body, &[secret]);
    assert!(!sanitized.contains(secret));
    assert!(!sanitized.contains("escaped\\\\credential"));
    assert!(sanitized.contains("[REDACTED]"));
}

async fn oversized_server(chunked: bool) -> Result<Url, Box<dyn std::error::Error>> {
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
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            MAX_TOKEN_RESPONSE_BYTES + 1
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
            for _ in 0..=(MAX_TOKEN_RESPONSE_BYTES / chunk.len()) {
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
            // 客户端错误断言覆盖该任务，不输出请求或响应内容。
        }
    });
    Ok(Url::parse(&format!("http://{address}/oauth/token"))?)
}
