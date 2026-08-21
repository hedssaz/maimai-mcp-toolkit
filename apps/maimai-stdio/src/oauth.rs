use std::{env, ffi::OsString};

use maimai_providers::{
    LxnsOAuthClient, OAuthConfig, OAuthError,
    lxns_oauth::{DEFAULT_AUTHORIZE_URL, DEFAULT_SCOPES, DEFAULT_TOKEN_URL},
};
use secrecy::SecretString;
use thiserror::Error;
use url::{ParseError, Url};

const CLIENT_ID_ENVS: [&str; 2] = ["LXNS_OAUTH_CLIENT_ID", "LXNS_CLIENT_ID"];
const CLIENT_SECRET_ENVS: [&str; 2] = ["LXNS_OAUTH_CLIENT_SECRET", "LXNS_CLIENT_SECRET"];
const REDIRECT_URI_ENVS: [&str; 2] = ["LXNS_OAUTH_REDIRECT_URI", "LXNS_REDIRECT_URI"];
const AUTHORIZE_URL_ENVS: [&str; 2] = ["LXNS_OAUTH_AUTHORIZE_URL", "LXNS_AUTHORIZE_URL"];
const TOKEN_URL_ENV: &str = "LXNS_OAUTH_TOKEN_URL";
const API_BASE_URL_ENV: &str = "LXNS_API_BASE_URL";
const SCOPES_ENVS: [&str; 2] = ["LXNS_OAUTH_SCOPES", "LXNS_SCOPES"];

struct OAuthClientSettings {
    client_id: Option<String>,
    client_secret: Option<SecretString>,
    redirect_uri: Option<Url>,
    authorize_url: Url,
    token_url: Url,
    scopes: Vec<String>,
}

impl OAuthClientSettings {
    fn from_env() -> Result<Self, LxnsConfigError> {
        Self::resolve(|name| env::var_os(name))
    }

    fn resolve(get: impl Fn(&str) -> Option<OsString>) -> Result<Self, LxnsConfigError> {
        let client_id = first_text(&get, &CLIENT_ID_ENVS)?;
        let client_secret = first_text(&get, &CLIENT_SECRET_ENVS)?.map(SecretString::from);
        let redirect_uri = first_text(&get, &REDIRECT_URI_ENVS)?
            .map(|value| parse_url(&value, REDIRECT_URI_ENVS[0]))
            .transpose()?;
        let authorize_url = parse_url(
            &first_text(&get, &AUTHORIZE_URL_ENVS)?
                .unwrap_or_else(|| DEFAULT_AUTHORIZE_URL.to_owned()),
            AUTHORIZE_URL_ENVS[0],
        )?;
        let token_url = token_url(&get)?;
        let scopes = parse_scopes(
            first_text(&get, &SCOPES_ENVS)?.unwrap_or_else(|| DEFAULT_SCOPES.join(" ")),
        )?;
        Ok(Self {
            client_id,
            client_secret,
            redirect_uri,
            authorize_url,
            token_url,
            scopes,
        })
    }

    fn into_client(self) -> Result<Option<LxnsOAuthClient>, LxnsConfigError> {
        let Some(client_id) = self.client_id else {
            return Ok(None);
        };
        let config = OAuthConfig::new(
            client_id,
            self.client_secret,
            self.redirect_uri,
            self.authorize_url,
            self.token_url,
            self.scopes,
        )?;
        Ok(Some(LxnsOAuthClient::new(config)?))
    }
}

pub(crate) fn public_oauth_client_from_env() -> Result<Option<LxnsOAuthClient>, LxnsConfigError> {
    OAuthClientSettings::from_env()?.into_client()
}

fn token_url(get: &impl Fn(&str) -> Option<OsString>) -> Result<Url, LxnsConfigError> {
    if let Some(value) = first_text(get, &[TOKEN_URL_ENV])? {
        return parse_url(&value, TOKEN_URL_ENV);
    }
    if let Some(api_base) = first_text(get, &[API_BASE_URL_ENV])? {
        let value = format!("{}/oauth/token", api_base.trim_end_matches('/'));
        return parse_url(&value, API_BASE_URL_ENV);
    }
    parse_url(DEFAULT_TOKEN_URL, TOKEN_URL_ENV)
}

fn first_text(
    get: &impl Fn(&str) -> Option<OsString>,
    names: &[&'static str],
) -> Result<Option<String>, LxnsConfigError> {
    for &name in names {
        if let Some(value) = optional_text(get(name), name)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn optional_text(
    value: Option<OsString>,
    name: &'static str,
) -> Result<Option<String>, LxnsConfigError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let value = value
        .into_string()
        .map_err(|_| LxnsConfigError::NonUnicodeEnvironment { name })?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().any(char::is_control) {
        return Err(LxnsConfigError::InvalidEnvironment { name });
    }
    Ok(Some(value))
}

fn parse_url(value: &str, name: &'static str) -> Result<Url, LxnsConfigError> {
    let url = Url::parse(value).map_err(|source| LxnsConfigError::InvalidUrl { name, source })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.fragment().is_some()
    {
        return Err(LxnsConfigError::InvalidEndpoint { name });
    }
    Ok(url)
}

fn parse_scopes(value: String) -> Result<Vec<String>, LxnsConfigError> {
    let scopes = value
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if scopes.is_empty() {
        return Err(LxnsConfigError::InvalidScopes);
    }
    Ok(scopes)
}

#[derive(Debug, Error)]
pub enum LxnsConfigError {
    #[error("环境变量 {name} 不是有效 Unicode 文本")]
    NonUnicodeEnvironment { name: &'static str },

    #[error("环境变量 {name} 含控制字符")]
    InvalidEnvironment { name: &'static str },

    #[error("环境变量 {name} 不是有效 URL：{source}")]
    InvalidUrl {
        name: &'static str,
        source: ParseError,
    },

    #[error("环境变量 {name} 必须是无 fragment 的完整 HTTP(S) URL")]
    InvalidEndpoint { name: &'static str },

    #[error("LXNS OAuth scopes 配置无效")]
    InvalidScopes,

    #[error("LXNS OAuth 配置无效：{0}")]
    Provider(#[from] OAuthError),
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, error::Error};

    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn missing_client_keeps_valid_public_defaults_without_creating_a_client()
    -> Result<(), Box<dyn Error>> {
        let settings = OAuthClientSettings::resolve(|_| None)?;
        assert!(settings.client_id.is_none());
        assert_eq!(settings.authorize_url.as_str(), DEFAULT_AUTHORIZE_URL);
        assert_eq!(settings.token_url.as_str(), DEFAULT_TOKEN_URL);
        assert_eq!(settings.scopes, DEFAULT_SCOPES);
        assert!(settings.into_client()?.is_none());
        Ok(())
    }

    #[test]
    fn oauth_names_take_priority_and_api_base_builds_token_url() -> Result<(), Box<dyn Error>> {
        let values = HashMap::from([
            ("LXNS_OAUTH_CLIENT_ID", OsString::from("preferred-client")),
            ("LXNS_CLIENT_ID", OsString::from("fallback-client")),
            (
                "LXNS_OAUTH_CLIENT_SECRET",
                OsString::from("secret-sentinel"),
            ),
            (
                "LXNS_OAUTH_REDIRECT_URI",
                OsString::from("https://example.test/lxns/callback"),
            ),
            (
                "LXNS_OAUTH_AUTHORIZE_URL",
                OsString::from("https://example.test/authorize"),
            ),
            (
                "LXNS_API_BASE_URL",
                OsString::from("https://example.test/api/v0/"),
            ),
            (
                "LXNS_OAUTH_SCOPES",
                OsString::from("read_player write_player"),
            ),
        ]);
        let settings = OAuthClientSettings::resolve(|name| values.get(name).cloned())?;
        let client = settings.into_client()?.ok_or("configured client missing")?;
        let config = client.config();
        assert_eq!(config.client_id(), "preferred-client");
        assert_eq!(
            config.client_secret().map(ExposeSecret::expose_secret),
            Some("secret-sentinel")
        );
        assert_eq!(
            config.token_url().as_str(),
            "https://example.test/api/v0/oauth/token"
        );
        assert!(!format!("{config:?}").contains("secret-sentinel"));
        Ok(())
    }

    #[test]
    fn malformed_urls_and_scopes_fail_without_a_client_id() {
        let values = HashMap::from([("LXNS_OAUTH_AUTHORIZE_URL", OsString::from("not a URL"))]);
        assert!(OAuthClientSettings::resolve(|name| values.get(name).cloned()).is_err());

        let values = HashMap::from([(
            "LXNS_OAUTH_SCOPES",
            OsString::from("read_player\nwrite_player"),
        )]);
        assert!(OAuthClientSettings::resolve(|name| values.get(name).cloned()).is_err());
    }
}
