use std::time::Duration;

use maimai_providers::{OAuthTokens, PkceVerifier};
use secrecy::SecretString;

use super::{OAuthService, OAuthServiceError, OAuthServiceErrorCode, OAuthSubject};

pub(super) struct PreparePokeSchedule {
    pub ttl_seconds: u64,
    pub now: i64,
    pub timeout: Option<Duration>,
}

pub(super) fn validate_timeout(timeout: Duration) -> Result<(), OAuthServiceError> {
    if timeout < Duration::from_millis(100) || timeout > Duration::from_secs(300) {
        return Err(OAuthServiceError::public(
            OAuthServiceErrorCode::InvalidInput,
            "timeout 必须是 0.1 到 300 秒。",
        ));
    }
    Ok(())
}

pub(super) async fn tokens(
    service: &OAuthService,
    subject: &OAuthSubject,
    generation: u64,
    code: &SecretString,
    verifier: &PkceVerifier,
    timeout: Option<Duration>,
) -> Result<OAuthTokens, OAuthServiceError> {
    let exchange = service
        .client()?
        .exchange_authorization_code(code, verifier);
    let result = match timeout {
        Some(timeout) => tokio::time::timeout(timeout, exchange)
            .await
            .map_err(|_| timeout_error()),
        None => Ok(exchange.await),
    };
    match result {
        Ok(Ok(tokens)) => Ok(tokens),
        Ok(Err(error)) => {
            release(service, subject, generation).await?;
            Err(error.into())
        }
        Err(error) => {
            release(service, subject, generation).await?;
            Err(error)
        }
    }
}

async fn release(
    service: &OAuthService,
    subject: &OAuthSubject,
    generation: u64,
) -> Result<(), OAuthServiceError> {
    service
        .store()
        .release_oauth_authorization_claim(subject.as_str(), generation)
        .await?;
    Ok(())
}

fn timeout_error() -> OAuthServiceError {
    OAuthServiceError::public(OAuthServiceErrorCode::Timeout, "落雪 OAuth 请求超时。")
}
