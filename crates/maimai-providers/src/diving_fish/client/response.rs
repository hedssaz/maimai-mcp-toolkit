use std::fmt;

use reqwest::{
    Response, StatusCode, Url,
    header::{HeaderMap, HeaderName, SET_COOKIE},
};
use secrecy::SecretString;
use serde_json::Value;

use super::super::redaction::{is_sensitive_name, sanitize_error_body};
use super::super::{
    DivingFishOperation, MAX_RESPONSE_BODY_BYTES, ProviderError, ProviderErrorCode,
};

pub struct DivingFishResponse {
    operation: DivingFishOperation,
    status: u16,
    url: Url,
    headers: HeaderMap,
    data: Option<Value>,
    text: Option<String>,
    jwt_token: Option<SecretString>,
}

impl DivingFishResponse {
    pub(super) fn new(
        operation: DivingFishOperation,
        status: u16,
        url: Url,
        headers: HeaderMap,
        data: Option<Value>,
        text: Option<String>,
        jwt_token: Option<SecretString>,
    ) -> Self {
        Self {
            operation,
            status,
            url,
            headers,
            data,
            text,
            jwt_token,
        }
    }

    pub fn operation(&self) -> DivingFishOperation {
        self.operation
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn jwt_token(&self) -> Option<&SecretString> {
        self.jwt_token.as_ref()
    }
}

impl fmt::Debug for DivingFishResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header_names = self
            .headers
            .keys()
            .map(HeaderName::as_str)
            .collect::<Vec<_>>();
        let endpoint = format!(
            "{}{}",
            self.url.origin().ascii_serialization(),
            self.url.path()
        );
        formatter
            .debug_struct("DivingFishResponse")
            .field("operation", &self.operation)
            .field("status", &self.status)
            .field("endpoint", &endpoint)
            .field("header_names", &header_names)
            .field("has_data", &self.data.is_some())
            .field("has_text", &self.text.is_some())
            .field("has_jwt_token", &self.jwt_token.is_some())
            .finish()
    }
}

pub(super) async fn decode(
    operation: DivingFishOperation,
    mut response: Response,
    redactions: &[String],
) -> Result<DivingFishResponse, ProviderError> {
    let status = response.status();
    let response_url = response.url().clone();
    let raw_response_headers = response.headers().clone();
    if status.is_redirection() {
        return Err(ProviderError::redirect(status.as_u16()));
    }
    let jwt_token = extract_jwt_token(&raw_response_headers);
    let response_headers = sanitized_response_headers(raw_response_headers);
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
    {
        return Err(body_too_large(status));
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .map_or(0, |length| length)
            .min(MAX_RESPONSE_BODY_BYTES),
    );
    while let Some(chunk) = response.chunk().await.map_err(read_error)? {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| body_too_large(status))?;
        if next_len > MAX_RESPONSE_BODY_BYTES {
            return Err(body_too_large(status));
        }
        bytes.extend_from_slice(&chunk);
    }
    let response_text = String::from_utf8_lossy(&bytes).into_owned();

    if !status.is_success() && status != StatusCode::NOT_MODIFIED {
        let body =
            (!response_text.is_empty()).then(|| sanitize_error_body(&response_text, redactions));
        return Err(ProviderError::http(status.as_u16(), body));
    }

    let (data, text) = parse_response_body(response_text);
    Ok(DivingFishResponse::new(
        operation,
        status.as_u16(),
        response_url,
        response_headers,
        data,
        text,
        jwt_token,
    ))
}

fn read_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::new(ProviderErrorCode::Timeout, "读取 Diving-Fish 响应超时")
    } else {
        ProviderError::new(ProviderErrorCode::Network, "读取 Diving-Fish 响应失败")
    }
}

fn body_too_large(status: StatusCode) -> ProviderError {
    ProviderError::body_too_large(status.as_u16())
}

fn extract_jwt_token(headers: &HeaderMap) -> Option<SecretString> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .map(str::trim)
        .find_map(|part| part.strip_prefix("jwt_token="))
        .filter(|value| !value.is_empty())
        .map(SecretString::from)
}

fn sanitized_response_headers(mut headers: HeaderMap) -> HeaderMap {
    let sensitive_names = headers
        .keys()
        .filter(|name| is_sensitive_name(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for name in sensitive_names {
        headers.remove(name);
    }
    headers
}

fn parse_response_body(body: String) -> (Option<Value>, Option<String>) {
    if body.is_empty() {
        return (None, None);
    }
    match serde_json::from_str(&body) {
        Ok(value) => (Some(value), None),
        Err(_) => (None, Some(body)),
    }
}
