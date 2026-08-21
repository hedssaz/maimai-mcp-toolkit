use std::{error::Error, io, time::Duration};

use maimai_providers::{LxnsOAuthClient, OAuthConfig};
use maimai_storage::{AuthorizationClaimResult, NewOAuthAuthorization, NewOAuthToken, StateStore};
use secrecy::SecretString;
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};
use url::Url;

use super::{
    OAuthService, OAuthServiceErrorCode, OAuthSubject, PokeContext, TrustedOAuthState,
    parse::parse_authorization_code,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

async fn serve_responses(
    bodies: Vec<(u16, String, Duration)>,
) -> Result<(Url, mpsc::Receiver<String>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (requests_tx, requests_rx) = mpsc::channel(bodies.len().max(1));
    tokio::spawn(async move {
        for (status, body, delay) in bodies {
            let accepted = listener.accept().await;
            let Ok((mut stream, _)) = accepted else {
                return;
            };
            let request = read_request(&mut stream).await;
            if let Ok(request) = request {
                let _ = requests_tx.send(request).await;
            }
            tokio::time::sleep(delay).await;
            let reason = if status >= 400 { "Error" } else { "OK" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    Ok((
        Url::parse(&format!("http://{address}/oauth/token"))?,
        requests_rx,
    ))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<String, io::Error> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "missing headers",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    String::from_utf8(bytes).map_err(io::Error::other)
}

fn client(token_url: Url) -> Result<LxnsOAuthClient, Box<dyn Error + Send + Sync>> {
    let config = OAuthConfig::new(
        "client-id",
        Some(SecretString::from("client-secret-sentinel".to_owned())),
        Some(Url::parse("https://bot.example.test/lxns/callback")?),
        Url::parse("https://maimai.example.test/oauth/authorize")?,
        token_url,
        vec![
            "write_player".to_owned(),
            "read_user_profile".to_owned(),
            "read_player".to_owned(),
        ],
    )?;
    Ok(LxnsOAuthClient::new(config)?.with_timeout(Duration::from_secs(2))?)
}

#[test]
fn submission_parser_rejects_ambiguous_and_unsafe_callbacks() -> TestResult {
    for invalid in [
        "code=first&code=second",
        "code=value&state=first&state=second",
        "code=value&error=first&error=second",
        "https://bot.example.test/callback?code=value&a=1&b=2&c=3&d=4&e=5&f=6&g=7&h=8",
        "ftp://bot.example.test/callback?code=value",
        "javascript://callback?code=value",
        "https://bot.example.test/callback",
        "https://bot.example.test/callback?code=value#state=hidden-state",
    ] {
        let error = parse_authorization_code(invalid)
            .err()
            .ok_or("unsafe callback was accepted")?;
        assert_eq!(error.code(), OAuthServiceErrorCode::InvalidInput);
    }
    let rejected = parse_authorization_code("error=access_denied")
        .err()
        .ok_or("provider rejection was accepted")?;
    assert_eq!(rejected.code(), OAuthServiceErrorCode::OAuthRejected);
    for accepted in [
        "raw-code",
        "code=assigned-code",
        "https://bot.example.test/callback?code=url-code&state=opaque-state",
    ] {
        assert!(parse_authorization_code(accepted).is_ok());
    }
    Ok(())
}

#[test]
fn opaque_subject_and_trusted_state_validation_match_public_contract() -> TestResult {
    let subject = OAuthSubject::new("subject-1")?;
    assert_eq!(subject.as_str(), "subject-1");
    assert!(OAuthSubject::new("subject\0bad").is_err());
    assert!(TrustedOAuthState::new("signed.subject-1.value", &subject).is_err());
    assert!(TrustedOAuthState::new("signed\0value", &subject).is_err());
    let state = TrustedOAuthState::new("signed.opaque.value", &subject)?;
    assert!(!format!("{state:?}").contains("signed.opaque.value"));
    let missing_group = PokeContext::new("napcat", "", "bot:20002")
        .err()
        .ok_or("empty conversation was accepted")?;
    assert_eq!(missing_group.to_string(), "缺少 conversation。");
    assert!(PokeContext::optional("napcat", "", "bot:20002")?.is_some());
    Ok(())
}

#[tokio::test]
async fn service_exchanges_real_http_code_and_persists_safe_binding() -> TestResult {
    let body = r#"{"success":true,"data":{"access_token":"access-secret-sentinel","refresh_token":"refresh-secret-sentinel","expires_in":900}}"#;
    let (token_url, mut requests) =
        serve_responses(vec![(200, body.to_owned(), Duration::ZERO)]).await?;
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let service = OAuthService::new(store.clone(), client(token_url)?);
    let subject = OAuthSubject::new("subject-1")?;
    let launch = service
        .authorization_url(subject.clone(), None, None, None, 600, 1_000)
        .await?;
    let generated_state = launch
        .url
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .ok_or("generated state missing")?;
    assert!(generated_state.len() >= 32);
    assert!(!generated_state.contains(subject.as_str()));
    assert!(!format!("{launch:?}").contains(&generated_state));
    let result = service
        .bind_code(
            subject.clone(),
            "code=oauth-code-sentinel",
            None,
            None,
            1_010,
        )
        .await?;
    assert_eq!(result.revision, 1);
    let request = requests.recv().await.ok_or("token request missing")?;
    assert!(request.contains("code=oauth-code-sentinel"));
    let status = service.status(subject, 1_010).await?;
    assert!(status.bound);
    let debug = format!("{launch:?} {result:?} {status:?}");
    assert!(!debug.contains("oauth-code-sentinel"));
    assert!(!debug.contains("access-secret-sentinel"));
    assert!(!debug.contains("refresh-secret-sentinel"));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn service_without_client_keeps_local_operations_and_types_remote_failures() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let service = OAuthService::without_client(store.clone());
    let cloned = service.clone();
    let subject = OAuthSubject::new("subject-1")?;

    assert!(!cloned.status(subject.clone(), 1_000).await?.bound);
    assert!(!service.unbind(subject.clone()).await?.changed);
    let missing = service
        .authorization_url(subject.clone(), None, None, None, 600, 1_000)
        .await
        .err()
        .ok_or("missing OAuth configuration was accepted")?;
    assert_eq!(missing.code(), OAuthServiceErrorCode::ConfigMissing);
    let missing = service
        .bind_code(subject.clone(), "code", None, None, 1_000)
        .await
        .err()
        .ok_or("code exchange without configuration was accepted")?;
    assert_eq!(missing.code(), OAuthServiceErrorCode::ConfigMissing);
    let missing = service
        .prepare_poke(
            subject.clone(),
            "code",
            None,
            PokeContext::new("napcat", "group:10001", "bot:20002")?,
            300,
            1_000,
        )
        .await
        .err()
        .ok_or("pending exchange without configuration was accepted")?;
    assert_eq!(missing.code(), OAuthServiceErrorCode::ConfigMissing);

    let authorization = NewOAuthAuthorization::new(
        subject.as_str().to_owned(),
        SecretString::from("state".to_owned()),
        SecretString::from(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~".to_owned(),
        ),
        None,
        1_000,
        2_000,
    );
    store.save_oauth_authorization(&authorization).await?;
    let claim = store
        .claim_oauth_authorization(subject.as_str(), None, None, None, 1_010)
        .await?;
    let AuthorizationClaimResult::Claimed(claim) = claim else {
        return Err("authorization was not claimed".into());
    };
    store
        .commit_oauth_authorization(
            subject.as_str(),
            claim.authorization.generation,
            &NewOAuthToken::new(
                SecretString::from("old-access".to_owned()),
                SecretString::from("old-refresh".to_owned()),
                "Bearer".to_owned(),
                None,
                "client-id".to_owned(),
                Some(1_050),
            ),
            1_010,
        )
        .await?
        .ok_or("token commit lost")?;
    let missing = service
        .access_token(subject, 1_100)
        .await
        .err()
        .ok_or("refresh without configuration was accepted")?;
    assert_eq!(missing.code(), OAuthServiceErrorCode::ConfigMissing);
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn per_call_exchange_timeout_releases_the_authorization_claim() -> TestResult {
    let body = r#"{"success":true,"data":{"access_token":"late-access","refresh_token":"late-refresh","expires_in":900}}"#;
    let (token_url, _requests) =
        serve_responses(vec![(200, body.to_owned(), Duration::from_millis(250))]).await?;
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let service = OAuthService::new(store.clone(), client(token_url)?);
    let subject = OAuthSubject::new("subject-timeout")?;
    service
        .authorization_url(subject.clone(), None, None, None, 600, 1_000)
        .await?;
    let error = service
        .bind_code_with_timeout(
            subject.clone(),
            "code=late-code",
            None,
            None,
            1_010,
            Duration::from_millis(100),
        )
        .await
        .err()
        .ok_or("slow exchange did not time out")?;
    assert_eq!(error.code(), OAuthServiceErrorCode::Timeout);
    let claim = store
        .claim_oauth_authorization(subject.as_str(), None, None, None, 1_011)
        .await?;
    let AuthorizationClaimResult::Claimed(claim) = claim else {
        return Err("timed-out exchange left authorization busy".into());
    };
    assert!(
        store
            .release_oauth_authorization_claim(subject.as_str(), claim.authorization.generation,)
            .await?
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn provider_timeout_maps_to_service_timeout_and_releases_claim() -> TestResult {
    let body = r#"{"access_token":"late-access","refresh_token":"late-refresh"}"#;
    let (token_url, _requests) =
        serve_responses(vec![(200, body.to_owned(), Duration::from_millis(250))]).await?;
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let oauth = client(token_url)?.with_timeout(Duration::from_millis(100))?;
    let service = OAuthService::new(store.clone(), oauth);
    let subject = OAuthSubject::new("subject-provider-timeout")?;
    service
        .authorization_url(subject.clone(), None, None, None, 600, 1_000)
        .await?;
    let error = service
        .bind_code(subject.clone(), "code=late-code", None, None, 1_010)
        .await
        .err()
        .ok_or("provider timeout was accepted")?;
    assert_eq!(error.code(), OAuthServiceErrorCode::Timeout);
    assert!(matches!(
        store
            .claim_oauth_authorization(subject.as_str(), None, None, None, 1_011)
            .await?,
        AuthorizationClaimResult::Claimed(_)
    ));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_refresh_recovers_invalid_grant_after_other_cas_wins() -> TestResult {
    let success =
        r#"{"access_token":"next-access","refresh_token":"next-refresh","expires_in":900}"#;
    let rejected = r#"{"error":"invalid_grant","error_description":"expired"}"#;
    let (token_url, _requests) = serve_responses(vec![
        (200, success.to_owned(), Duration::ZERO),
        (400, rejected.to_owned(), Duration::from_millis(150)),
    ])
    .await?;
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    let authorization = NewOAuthAuthorization::new(
        "subject-1".to_owned(),
        SecretString::from("state".to_owned()),
        SecretString::from(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~".to_owned(),
        ),
        None,
        1_000,
        2_000,
    );
    store.save_oauth_authorization(&authorization).await?;
    let claim = store
        .claim_oauth_authorization("subject-1", None, None, None, 1_010)
        .await?;
    let AuthorizationClaimResult::Claimed(claim) = claim else {
        return Err("authorization was not claimed".into());
    };
    store
        .commit_oauth_authorization(
            "subject-1",
            claim.authorization.generation,
            &NewOAuthToken::new(
                SecretString::from("old-access".to_owned()),
                SecretString::from("old-refresh".to_owned()),
                "Bearer".to_owned(),
                None,
                "client-id".to_owned(),
                Some(1_050),
            ),
            1_010,
        )
        .await?
        .ok_or("token commit lost")?;
    let service = OAuthService::new(store.clone(), client(token_url)?);
    let first_subject = OAuthSubject::new("subject-1")?;
    let second_subject = first_subject.clone();
    let (first, second) = tokio::join!(
        service.access_token(first_subject, 1_100),
        service.access_token(second_subject, 1_100)
    );
    assert_eq!(first?.generation(), 2);
    assert_eq!(second?.generation(), 2);
    let stored = store
        .oauth_token("subject-1")
        .await?
        .ok_or("refreshed token missing")?;
    assert_eq!(stored.generation, 2);
    store.close().await;
    Ok(())
}
