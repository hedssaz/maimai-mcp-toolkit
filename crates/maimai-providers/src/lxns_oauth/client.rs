use std::time::Duration;

use reqwest::{Client, StatusCode, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{
    AuthorizationRequest, MAX_TOKEN_RESPONSE_BYTES, OAuthConfig, OAuthError, OAuthErrorCode,
    OAuthState, OAuthTokens, PkceVerifier,
    config::validate_scopes,
    redaction::sanitize_error_body,
    tokens::{SuccessfulResponse, TokenWire},
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_AUTHORIZATION_CODE_BYTES: usize = 2_048;
const MAX_REFRESH_TOKEN_BYTES: usize = 16_384;
const MIN_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_TIMEOUT: Duration = Duration::from_secs(300);

pub struct LxnsOAuthClient {
    http: Client,
    config: OAuthConfig,
    timeout: Duration,
}

impl LxnsOAuthClient {
    pub fn new(config: OAuthConfig) -> Result<Self, OAuthError> {
        let http = Client::builder()
            .user_agent("maimai-providers/0.1.0")
            .redirect(Policy::none())
            .build()
            .map_err(|_| {
                OAuthError::new(
                    OAuthErrorCode::InvalidConfiguration,
                    "OAuth HTTP client 创建失败",
                )
            })?;
        Ok(Self {
            http,
            config,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self, OAuthError> {
        if !(MIN_TIMEOUT..=MAX_TIMEOUT).contains(&timeout) {
            return Err(OAuthError::new(
                OAuthErrorCode::InvalidConfiguration,
                "OAuth HTTP timeout 必须在 0.1 到 300 秒之间",
            ));
        }
        self.timeout = timeout;
        Ok(self)
    }

    pub fn config(&self) -> &OAuthConfig {
        &self.config
    }

    pub fn authorization_request(&self) -> Result<AuthorizationRequest, OAuthError> {
        self.authorization_request_with(None, None)
    }

    pub fn authorization_request_with(
        &self,
        state: Option<OAuthState>,
        scopes: Option<&[String]>,
    ) -> Result<AuthorizationRequest, OAuthError> {
        let state = match state {
            Some(state) => state,
            None => OAuthState::generate()?,
        };
        let code_verifier = PkceVerifier::generate()?;
        let challenge = code_verifier.challenge();
        let selected_scopes = scopes.unwrap_or_else(|| self.config.scopes());
        validate_scopes(selected_scopes, OAuthErrorCode::InvalidRequest)?;
        let scope = selected_scopes.join(" ");
        let mut url = self.config.authorize_url().clone();
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("response_type", "code")
                .append_pair("client_id", self.config.client_id())
                .append_pair("scope", &scope)
                .append_pair("state", state.secret().expose_secret())
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256");
            if let Some(redirect_uri) = self.config.redirect_uri() {
                query.append_pair("redirect_uri", redirect_uri.as_str());
            }
        }

        Ok(AuthorizationRequest {
            url,
            state,
            code_verifier,
        })
    }

    pub async fn exchange_authorization_code(
        &self,
        code: &SecretString,
        code_verifier: &PkceVerifier,
    ) -> Result<OAuthTokens, OAuthError> {
        validate_secret(code, MAX_AUTHORIZATION_CODE_BYTES, "authorization code")?;
        let client_secret = self.config.client_secret().map(ExposeSecret::expose_secret);
        let form = AuthorizationCodeForm {
            grant_type: "authorization_code",
            client_id: self.config.client_id(),
            client_secret,
            code: code.expose_secret(),
            code_verifier: code_verifier.secret().expose_secret(),
            redirect_uri: self.config.redirect_uri().map(Url::as_str),
        };
        let secrets = [
            code.expose_secret(),
            code_verifier.secret().expose_secret(),
            client_secret.unwrap_or_default(),
        ];
        self.request_tokens(&form, &secrets, None).await
    }

    pub async fn refresh_token(
        &self,
        refresh_token: &SecretString,
    ) -> Result<OAuthTokens, OAuthError> {
        validate_secret(refresh_token, MAX_REFRESH_TOKEN_BYTES, "refresh token")?;
        let client_secret = self.config.client_secret().map(ExposeSecret::expose_secret);
        let form = RefreshTokenForm {
            grant_type: "refresh_token",
            client_id: self.config.client_id(),
            client_secret,
            refresh_token: refresh_token.expose_secret(),
        };
        let secrets = [
            refresh_token.expose_secret(),
            client_secret.unwrap_or_default(),
        ];
        self.request_tokens(&form, &secrets, Some(refresh_token.expose_secret()))
            .await
    }

    async fn request_tokens<T: Serialize + ?Sized>(
        &self,
        form: &T,
        secrets: &[&str],
        previous_refresh_token: Option<&str>,
    ) -> Result<OAuthTokens, OAuthError> {
        let mut response = self
            .http
            .post(self.config.token_url().clone())
            .timeout(self.timeout)
            .form(form)
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status();
        let body = read_token_body(&mut response, status).await?;
        let response_status = serde_json::from_slice::<ResponseStatus>(&body).ok();
        let rejected_by_body = response_status
            .as_ref()
            .is_some_and(|payload| payload.success == Some(false) || payload.error.is_some());

        if !status.is_success() || rejected_by_body {
            return Err(rejected_response(
                status,
                &body,
                response_status.as_ref(),
                secrets,
            ));
        }

        let wire = serde_json::from_slice::<SuccessfulResponse>(&body).map_err(|_| {
            invalid_token_response(status, String::from_utf8_lossy(&body).as_ref(), secrets)
        })?;
        let token = wire.into_token();
        validate_token_payload(&token, status, &body, secrets)?;

        if previous_refresh_token
            .is_some_and(|previous| token.refresh_token.expose_secret() == previous)
        {
            return Err(OAuthError::response(
                OAuthErrorCode::RefreshTokenNotRotated,
                "OAuth refresh token 未轮换",
                status.as_u16(),
                None,
            ));
        }

        Ok(OAuthTokens {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            token_type: token
                .token_type
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Bearer".to_owned()),
            scope: token.scope.filter(|value| !value.trim().is_empty()),
            expires_in: token.expires_in.filter(|seconds| *seconds > 0),
        })
    }
}

async fn read_token_body(
    response: &mut reqwest::Response,
    status: StatusCode,
) -> Result<Vec<u8>, OAuthError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_TOKEN_RESPONSE_BYTES as u64)
    {
        return Err(token_body_too_large(status));
    }
    let capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or_default()
        .min(MAX_TOKEN_RESPONSE_BYTES);
    let mut bytes = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| token_body_too_large(status))?;
        if next_len > MAX_TOKEN_RESPONSE_BYTES {
            return Err(token_body_too_large(status));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn token_body_too_large(status: StatusCode) -> OAuthError {
    OAuthError::response(
        OAuthErrorCode::InvalidTokenResponse,
        "OAuth token 响应超过大小限制",
        status.as_u16(),
        None,
    )
}

#[derive(Serialize)]
struct AuthorizationCodeForm<'a> {
    grant_type: &'static str,
    client_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<&'a str>,
    code: &'a str,
    code_verifier: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_uri: Option<&'a str>,
}

#[derive(Serialize)]
struct RefreshTokenForm<'a> {
    grant_type: &'static str,
    client_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<&'a str>,
    refresh_token: &'a str,
}

#[derive(Deserialize)]
struct ResponseStatus {
    success: Option<bool>,
    error: Option<String>,
}

fn validate_secret(secret: &SecretString, max_bytes: usize, name: &str) -> Result<(), OAuthError> {
    let value = secret.expose_secret();
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(OAuthError::new(
            OAuthErrorCode::InvalidRequest,
            format!("OAuth {name} 格式不正确"),
        ));
    }
    Ok(())
}

fn validate_token_payload(
    token: &TokenWire,
    status: StatusCode,
    body: &[u8],
    secrets: &[&str],
) -> Result<(), OAuthError> {
    if token.access_token.expose_secret().is_empty()
        || token.refresh_token.expose_secret().is_empty()
    {
        return Err(invalid_token_response(
            status,
            String::from_utf8_lossy(body).as_ref(),
            secrets,
        ));
    }
    Ok(())
}

fn rejected_response(
    status: StatusCode,
    body: &[u8],
    response_status: Option<&ResponseStatus>,
    secrets: &[&str],
) -> OAuthError {
    let code = match response_status.and_then(|payload| payload.error.as_deref()) {
        Some("invalid_grant") => OAuthErrorCode::InvalidGrant,
        _ if status == StatusCode::UNAUTHORIZED => OAuthErrorCode::Unauthorized,
        _ => OAuthErrorCode::Http,
    };
    let message = match code {
        OAuthErrorCode::InvalidGrant => "OAuth authorization grant 已失效",
        OAuthErrorCode::Unauthorized => "OAuth token 请求未获授权",
        _ => "OAuth token 请求被拒绝",
    };
    let text = String::from_utf8_lossy(body);
    let sanitized = (!text.is_empty()).then(|| sanitize_error_body(&text, secrets));
    OAuthError::response(code, message, status.as_u16(), sanitized)
}

fn invalid_token_response(status: StatusCode, body: &str, secrets: &[&str]) -> OAuthError {
    let sanitized = (!body.is_empty()).then(|| sanitize_error_body(body, secrets));
    OAuthError::response(
        OAuthErrorCode::InvalidTokenResponse,
        "OAuth token 响应格式不正确",
        status.as_u16(),
        sanitized,
    )
}

fn request_error(error: reqwest::Error) -> OAuthError {
    if error.is_timeout() {
        OAuthError::new(OAuthErrorCode::Timeout, "OAuth token 请求超时")
    } else {
        OAuthError::new(OAuthErrorCode::Network, "OAuth token 网络请求失败")
    }
}
