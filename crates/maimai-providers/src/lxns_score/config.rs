use std::{fmt, time::Duration};

use secrecy::SecretString;
use url::Url;

use super::{LxnsScoreError, LxnsScoreErrorCode};

pub const DEFAULT_LXNS_SCORE_BASE_URL: &str = "https://maimai.lxns.net/api/v0/";
const MIN_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_TIMEOUT: Duration = Duration::from_secs(300);

pub struct LxnsScoreConfig {
    base_url: Url,
    timeout: Duration,
    access_token: SecretString,
}

impl LxnsScoreConfig {
    pub fn new(
        base_url: Url,
        timeout: Duration,
        access_token: SecretString,
    ) -> Result<Self, LxnsScoreError> {
        let (base_url, timeout) = Self::validate_endpoint(base_url, timeout)?;
        Ok(Self::from_validated(base_url, timeout, access_token))
    }

    pub(super) fn validate_endpoint(
        mut base_url: Url,
        timeout: Duration,
    ) -> Result<(Url, Duration), LxnsScoreError> {
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(LxnsScoreError::new(
                LxnsScoreErrorCode::InvalidConfiguration,
                "LXNS score base URL 格式不正确",
            ));
        }
        if !(MIN_TIMEOUT..=MAX_TIMEOUT).contains(&timeout) {
            return Err(LxnsScoreError::new(
                LxnsScoreErrorCode::InvalidConfiguration,
                "LXNS score timeout 必须在 0.1 到 300 秒之间",
            ));
        }
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        Ok((base_url, timeout))
    }

    pub(super) fn from_validated(
        base_url: Url,
        timeout: Duration,
        access_token: SecretString,
    ) -> Self {
        Self {
            base_url,
            timeout,
            access_token,
        }
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn access_token(&self) -> &SecretString {
        &self.access_token
    }

    pub(super) fn endpoint(&self, path: &str) -> Result<Url, LxnsScoreError> {
        self.base_url.join(path).map_err(|_| {
            LxnsScoreError::new(
                LxnsScoreErrorCode::InvalidConfiguration,
                "LXNS score endpoint URL 构造失败",
            )
        })
    }

    pub(super) fn into_parts(self) -> (Url, Duration, SecretString) {
        (self.base_url, self.timeout, self.access_token)
    }
}

impl fmt::Debug for LxnsScoreConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LxnsScoreConfig")
            .field(
                "base_url",
                &format_args!(
                    "{}{}",
                    self.base_url.origin().ascii_serialization(),
                    self.base_url.path()
                ),
            )
            .field("timeout", &self.timeout)
            .field("has_access_token", &true)
            .finish()
    }
}
