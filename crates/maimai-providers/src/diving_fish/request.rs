use std::{collections::BTreeMap, fmt, time::Duration};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use super::{DivingFishCredentials, DivingFishOperation, ProviderError, ProviderErrorCode};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryValue {
    Single(String),
    Multiple(Vec<String>),
}

impl From<String> for QueryValue {
    fn from(value: String) -> Self {
        Self::Single(value)
    }
}

impl From<&str> for QueryValue {
    fn from(value: &str) -> Self {
        Self::Single(value.to_owned())
    }
}

impl From<Vec<String>> for QueryValue {
    fn from(value: Vec<String>) -> Self {
        Self::Multiple(value)
    }
}

pub struct DivingFishRequest {
    pub(super) operation: DivingFishOperation,
    pub(super) query: BTreeMap<String, QueryValue>,
    pub(super) body: Option<Value>,
    pub(super) raw_body: Option<String>,
    pub(super) headers: HeaderMap,
    pub(super) if_none_match: Option<HeaderValue>,
    pub(super) timeout: Duration,
    pub(super) confirm: Option<DivingFishOperation>,
    pub(super) credentials: DivingFishCredentials,
}

impl DivingFishRequest {
    pub fn new(operation: DivingFishOperation) -> Self {
        Self {
            operation,
            query: BTreeMap::new(),
            body: None,
            raw_body: None,
            headers: HeaderMap::new(),
            if_none_match: None,
            timeout: DEFAULT_TIMEOUT,
            confirm: None,
            credentials: DivingFishCredentials::default(),
        }
    }

    pub fn operation(&self) -> DivingFishOperation {
        self.operation
    }

    pub fn with_query(mut self, key: impl Into<String>, value: impl Into<QueryValue>) -> Self {
        self.query.insert(key.into(), value.into());
        self
    }

    pub fn with_body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_raw_body(mut self, raw_body: impl Into<String>) -> Self {
        self.raw_body = Some(raw_body.into());
        self
    }

    pub fn try_with_headers(
        mut self,
        headers: BTreeMap<String, String>,
    ) -> Result<Self, ProviderError> {
        for (name, value) in headers {
            if super::redaction::is_sensitive_name(&name) {
                return Err(ProviderError::new(
                    ProviderErrorCode::InvalidRequest,
                    "自定义 headers 不允许覆盖凭据头",
                ));
            }
            let name = HeaderName::try_from(name).map_err(|_| {
                ProviderError::new(ProviderErrorCode::InvalidRequest, "header 名称格式不正确")
            })?;
            let value = HeaderValue::from_str(&value).map_err(|_| {
                ProviderError::new(ProviderErrorCode::InvalidRequest, "header 值格式不正确")
            })?;
            self.headers.insert(name, value);
        }
        Ok(self)
    }

    pub fn try_with_if_none_match(mut self, etag: &str) -> Result<Self, ProviderError> {
        let value = HeaderValue::from_str(etag).map_err(|_| {
            ProviderError::new(ProviderErrorCode::InvalidRequest, "ETag 格式不正确")
        })?;
        self.if_none_match = Some(value);
        Ok(self)
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn confirm(mut self, operation: DivingFishOperation) -> Self {
        self.confirm = Some(operation);
        self
    }

    pub fn with_credentials(mut self, credentials: DivingFishCredentials) -> Self {
        self.credentials = credentials;
        self
    }
}

impl fmt::Debug for DivingFishRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header_names = self
            .headers
            .keys()
            .map(HeaderName::as_str)
            .collect::<Vec<_>>();
        let query_keys = self.query.keys().collect::<Vec<_>>();
        formatter
            .debug_struct("DivingFishRequest")
            .field("operation", &self.operation)
            .field("query_keys", &query_keys)
            .field("has_body", &self.body.is_some())
            .field("has_raw_body", &self.raw_body.is_some())
            .field("header_names", &header_names)
            .field("has_if_none_match", &self.if_none_match.is_some())
            .field("timeout", &self.timeout)
            .field("confirm", &self.confirm)
            .field("credentials", &self.credentials)
            .finish()
    }
}
