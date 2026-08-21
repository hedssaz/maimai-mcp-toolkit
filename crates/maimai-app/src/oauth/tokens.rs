use maimai_providers::OAuthTokens;
use maimai_storage::{NewOAuthToken, OAuthTokenRecord};

use super::{AccessGrant, OAuthServiceError, OAuthServiceErrorCode, OAuthSubject};

pub(super) fn new_stored_token(
    tokens: &OAuthTokens,
    client_id: &str,
    now: i64,
) -> Result<NewOAuthToken, OAuthServiceError> {
    let expires_at = tokens
        .expires_in()
        .map(|seconds| {
            i64::try_from(seconds)
                .ok()
                .and_then(|seconds| now.checked_add(seconds))
                .ok_or_else(|| {
                    OAuthServiceError::public(
                        OAuthServiceErrorCode::Provider,
                        "落雪 OAuth token 过期时间超出支持范围。",
                    )
                })
        })
        .transpose()?;
    Ok(NewOAuthToken::new(
        tokens.access_token().clone(),
        tokens.refresh_token().clone(),
        tokens.token_type().to_owned(),
        tokens.scope().map(str::to_owned),
        client_id.to_owned(),
        expires_at,
    ))
}

pub(super) fn access_grant(subject: OAuthSubject, record: OAuthTokenRecord) -> AccessGrant {
    let generation = record.generation;
    let expires_at = record.expires_at;
    let access_token = record.into_access_token();
    AccessGrant::new(subject, generation, access_token, expires_at)
}
