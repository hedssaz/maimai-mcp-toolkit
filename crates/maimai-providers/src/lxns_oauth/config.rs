use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::{OAuthError, OAuthErrorCode, redaction::safe_endpoint};

pub const DEFAULT_AUTHORIZE_URL: &str = "https://maimai.lxns.net/oauth/authorize";
pub const DEFAULT_TOKEN_URL: &str = "https://maimai.lxns.net/api/v0/oauth/token";
pub const DEFAULT_SCOPES: [&str; 3] = ["write_player", "read_user_profile", "read_player"];

pub struct OAuthConfig {
    client_id: String,
    client_secret: Option<SecretString>,
    redirect_uri: Option<Url>,
    authorize_url: Url,
    token_url: Url,
    scopes: Vec<String>,
}

impl OAuthConfig {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: Option<SecretString>,
        redirect_uri: Option<Url>,
        authorize_url: Url,
        token_url: Url,
        scopes: Vec<String>,
    ) -> Result<Self, OAuthError> {
        let client_id = client_id.into();
        validate_text(&client_id, "OAuth client_id")?;
        if client_secret.as_ref().is_some_and(|secret| {
            let value = secret.expose_secret();
            value.is_empty() || value.chars().any(char::is_control)
        }) {
            return Err(OAuthError::new(
                OAuthErrorCode::InvalidConfiguration,
                "OAuth client_secret 格式不正确",
            ));
        }
        validate_endpoint(&authorize_url, "authorization")?;
        validate_endpoint(&token_url, "token")?;
        if let Some(redirect_uri) = redirect_uri.as_ref() {
            validate_http_url(redirect_uri, "redirect_uri")?;
            if redirect_uri.fragment().is_some() {
                return Err(OAuthError::new(
                    OAuthErrorCode::InvalidConfiguration,
                    "OAuth redirect_uri 不应包含 fragment",
                ));
            }
        }
        validate_scopes(&scopes, OAuthErrorCode::InvalidConfiguration)?;

        Ok(Self {
            client_id,
            client_secret,
            redirect_uri,
            authorize_url,
            token_url,
            scopes,
        })
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn client_secret(&self) -> Option<&SecretString> {
        self.client_secret.as_ref()
    }

    pub fn redirect_uri(&self) -> Option<&Url> {
        self.redirect_uri.as_ref()
    }

    pub fn authorize_url(&self) -> &Url {
        &self.authorize_url
    }

    pub fn token_url(&self) -> &Url {
        &self.token_url
    }

    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

impl fmt::Debug for OAuthConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthConfig")
            .field("client_id", &self.client_id)
            .field("has_client_secret", &self.client_secret.is_some())
            .field(
                "redirect_uri",
                &self.redirect_uri.as_ref().map(safe_endpoint),
            )
            .field("authorize_url", &safe_endpoint(&self.authorize_url))
            .field("token_url", &safe_endpoint(&self.token_url))
            .field("scopes", &self.scopes)
            .finish()
    }
}

fn validate_text(value: &str, field: &str) -> Result<(), OAuthError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(OAuthError::new(
            OAuthErrorCode::InvalidConfiguration,
            format!("{field} 格式不正确"),
        ));
    }
    Ok(())
}

pub(super) fn validate_scopes(
    scopes: &[String],
    error_code: OAuthErrorCode,
) -> Result<(), OAuthError> {
    if scopes.is_empty()
        || scopes.iter().any(|scope| {
            scope.is_empty()
                || scope
                    .chars()
                    .any(|character| character.is_control() || character.is_whitespace())
        })
    {
        return Err(OAuthError::new(error_code, "OAuth scope 格式不正确"));
    }
    Ok(())
}

fn validate_endpoint(url: &Url, name: &str) -> Result<(), OAuthError> {
    validate_http_url(url, name)?;
    if url.fragment().is_some() {
        return Err(OAuthError::new(
            OAuthErrorCode::InvalidConfiguration,
            format!("OAuth {name} endpoint 不应包含 fragment"),
        ));
    }
    Ok(())
}

fn validate_http_url(url: &Url, name: &str) -> Result<(), OAuthError> {
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(OAuthError::new(
            OAuthErrorCode::InvalidConfiguration,
            format!("OAuth {name} URL 必须是完整的 HTTP(S) URL"),
        ));
    }
    Ok(())
}
