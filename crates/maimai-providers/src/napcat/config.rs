use std::{fmt, time::Duration};

use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::{NapCatError, NapCatErrorCode, redaction::safe_endpoint};

pub const DEFAULT_NAPCAT_BASE_URL: &str = "http://napcat:3000/";

#[derive(Clone)]
pub struct NapCatConfig {
    base_url: Url,
    timeout: Duration,
    access_token: Option<SecretString>,
}

impl NapCatConfig {
    pub fn new(
        mut base_url: Url,
        timeout: Duration,
        access_token: Option<SecretString>,
    ) -> Result<Self, NapCatError> {
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(NapCatError::new(
                NapCatErrorCode::InvalidConfiguration,
                "NapCat base URL 格式不正确",
            ));
        }
        if timeout.is_zero() {
            return Err(NapCatError::new(
                NapCatErrorCode::InvalidConfiguration,
                "NapCat timeout 必须大于零",
            ));
        }
        if access_token.as_ref().is_some_and(|secret| {
            let value = secret.expose_secret();
            value.trim().is_empty()
                || value.trim() != value
                || value
                    .chars()
                    .any(|character| character.is_control() || character.is_whitespace())
        }) {
            return Err(NapCatError::new(
                NapCatErrorCode::InvalidConfiguration,
                "NapCat access token 格式不正确",
            ));
        }

        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }

        Ok(Self {
            base_url,
            timeout,
            access_token,
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn access_token(&self) -> Option<&SecretString> {
        self.access_token.as_ref()
    }

    pub(super) fn endpoint(&self, path: &str) -> Result<Url, NapCatError> {
        self.base_url.join(path).map_err(|_| {
            NapCatError::new(
                NapCatErrorCode::InvalidConfiguration,
                "NapCat endpoint URL 构造失败",
            )
        })
    }
}

impl fmt::Debug for NapCatConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NapCatConfig")
            .field("base_url", &safe_endpoint(&self.base_url))
            .field("timeout", &self.timeout)
            .field("has_access_token", &self.access_token.is_some())
            .finish()
    }
}
