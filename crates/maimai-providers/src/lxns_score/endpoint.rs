use std::{fmt, time::Duration};

use reqwest::{Client, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::{LxnsScoreClient, LxnsScoreConfig, LxnsScoreError, LxnsScoreErrorCode};

#[derive(Clone)]
pub struct LxnsScoreEndpoint {
    http: Client,
    base_url: Url,
    timeout: Duration,
}

impl LxnsScoreEndpoint {
    pub fn new(base_url: Url, timeout: Duration) -> Result<Self, LxnsScoreError> {
        let (base_url, timeout) = LxnsScoreConfig::validate_endpoint(base_url, timeout)?;
        let http = Client::builder()
            .user_agent("maimai-lxns-provider/0.1.0")
            .redirect(Policy::none())
            .build()
            .map_err(|_| {
                LxnsScoreError::new(
                    LxnsScoreErrorCode::InvalidConfiguration,
                    "LXNS score HTTP client 创建失败",
                )
            })?;
        Ok(Self {
            http,
            base_url,
            timeout,
        })
    }

    pub fn authorize(&self, access_token: SecretString) -> Result<LxnsScoreClient, LxnsScoreError> {
        validate_access_token(&access_token)?;
        Ok(LxnsScoreClient::authorized(
            self.clone(),
            LxnsScoreConfig::from_validated(self.base_url.clone(), self.timeout, access_token),
        ))
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(super) fn http(&self) -> &Client {
        &self.http
    }
}

impl fmt::Debug for LxnsScoreEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LxnsScoreEndpoint")
            .field(
                "base_url",
                &format_args!(
                    "{}{}",
                    self.base_url.origin().ascii_serialization(),
                    self.base_url.path()
                ),
            )
            .field("timeout", &self.timeout)
            .finish()
    }
}

fn validate_access_token(access_token: &SecretString) -> Result<(), LxnsScoreError> {
    let value = access_token.expose_secret();
    if value.is_empty()
        || value.trim() != value
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(LxnsScoreError::new(
            LxnsScoreErrorCode::InvalidConfiguration,
            "LXNS access token 格式不正确",
        ));
    }
    Ok(())
}
