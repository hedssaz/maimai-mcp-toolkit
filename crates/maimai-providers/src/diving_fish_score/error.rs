use std::{error::Error, fmt};

use crate::{ProviderError, ProviderErrorCode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DivingFishScoreErrorCode {
    InvalidSelector,
    InvalidRequest,
    InvalidResponse,
    Provider(ProviderErrorCode),
}

pub struct DivingFishScoreError {
    code: DivingFishScoreErrorCode,
    message: String,
    status: Option<u16>,
    body: Option<String>,
    source: Option<ProviderError>,
}

impl DivingFishScoreError {
    pub(crate) fn invalid_selector(message: impl Into<String>) -> Self {
        Self::new(DivingFishScoreErrorCode::InvalidSelector, message)
    }

    pub(crate) fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(DivingFishScoreErrorCode::InvalidRequest, message)
    }

    pub(crate) fn invalid_response(message: impl Into<String>) -> Self {
        Self::new(DivingFishScoreErrorCode::InvalidResponse, message)
    }

    pub(crate) fn provider(error: ProviderError, redactions: &[&str]) -> Self {
        let body = error.body().map(|body| {
            redactions.iter().fold(body.to_owned(), |body, secret| {
                if secret.is_empty() {
                    body
                } else {
                    body.replace(secret, "[REDACTED]")
                }
            })
        });
        Self {
            code: DivingFishScoreErrorCode::Provider(error.code()),
            message: error.to_string(),
            status: error.status(),
            body,
            source: Some(error),
        }
    }

    pub fn code(&self) -> DivingFishScoreErrorCode {
        self.code
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    fn new(code: DivingFishScoreErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            body: None,
            source: None,
        }
    }
}

impl fmt::Display for DivingFishScoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl fmt::Debug for DivingFishScoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DivingFishScoreError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("status", &self.status)
            .field("has_body", &self.body.is_some())
            .field("has_source", &self.source.is_some())
            .finish()
    }
}

impl Error for DivingFishScoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|error| error as &dyn Error)
    }
}
