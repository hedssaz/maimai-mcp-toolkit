use reqwest::{Client, Method, RequestBuilder};
use secrecy::ExposeSecret;
use serde::Serialize;
use serde_json::Value;

use super::{
    LxnsScoreConfig, LxnsScoreError, LxnsScoreErrorCode, MAX_RESPONSE_BODY_BYTES, PlayerUpdate,
    ScoreUpload, redaction::sanitize_error_body,
};

pub(super) enum RequestBody<'a> {
    Player(&'a PlayerUpdate),
    Scores(&'a [ScoreUpload]),
}

#[derive(Debug)]
pub(super) struct ResponsePayload {
    pub value: Value,
    pub status: u16,
    pub sanitized_body: String,
}

pub(super) async fn send(
    http: &Client,
    config: &LxnsScoreConfig,
    method: Method,
    path: &str,
    query: Option<(&str, u32)>,
    body: Option<RequestBody<'_>>,
) -> Result<ResponsePayload, LxnsScoreError> {
    let mut url = config.endpoint(path)?;
    if let Some((name, value)) = query {
        url.query_pairs_mut().append_pair(name, &value.to_string());
    }
    let access_token = config.access_token().expose_secret();
    let mut request = http
        .request(method, url)
        .timeout(config.timeout())
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", "maimai-lxns-provider/0.1.0")
        .bearer_auth(access_token);
    request = attach_body(request, body)?;

    let mut response = request.send().await.map_err(request_error)?;
    let status = response.status();
    let bytes = read_body(&mut response, status.as_u16()).await?;
    let raw = String::from_utf8_lossy(&bytes);
    let sanitized = sanitize_error_body(&raw, access_token);
    let value = serde_json::from_slice::<Value>(&bytes).map_err(|_| {
        LxnsScoreError::response(
            LxnsScoreErrorCode::InvalidJson,
            "LXNS score 接口返回非 JSON 内容",
            status.as_u16(),
            (!sanitized.is_empty()).then_some(sanitized.clone()),
        )
    })?;

    if status.as_u16() == 401 {
        return Err(LxnsScoreError::response(
            LxnsScoreErrorCode::Unauthorized,
            "LXNS access token 未获授权",
            401,
            Some(sanitized),
        ));
    }
    if status.is_redirection() {
        return Err(LxnsScoreError::response(
            LxnsScoreErrorCode::Redirect,
            "LXNS score 接口返回重定向",
            status.as_u16(),
            Some(sanitized),
        ));
    }
    if !status.is_success() {
        return Err(LxnsScoreError::response(
            LxnsScoreErrorCode::Http,
            format!("LXNS score 请求失败：HTTP {}", status.as_u16()),
            status.as_u16(),
            Some(sanitized),
        ));
    }
    if value.as_object().and_then(|object| object.get("success")) == Some(&Value::Bool(false)) {
        return Err(LxnsScoreError::response(
            LxnsScoreErrorCode::Api,
            "LXNS score 接口返回业务错误",
            status.as_u16(),
            Some(sanitized),
        ));
    }
    let payload = value
        .as_object()
        .and_then(|object| object.get("data"))
        .cloned()
        .unwrap_or(value);
    Ok(ResponsePayload {
        value: payload,
        status: status.as_u16(),
        sanitized_body: sanitized,
    })
}

async fn read_body(
    response: &mut reqwest::Response,
    status: u16,
) -> Result<Vec<u8>, LxnsScoreError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
    {
        return Err(response_too_large(status));
    }
    let capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or_default()
        .min(MAX_RESPONSE_BODY_BYTES);
    let mut bytes = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| response_too_large(status))?;
        if next_len > MAX_RESPONSE_BODY_BYTES {
            return Err(response_too_large(status));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn response_too_large(status: u16) -> LxnsScoreError {
    LxnsScoreError::response(
        LxnsScoreErrorCode::ResponseTooLarge,
        "LXNS score 响应超过大小限制",
        status,
        None,
    )
}

fn attach_body(
    request: RequestBuilder,
    body: Option<RequestBody<'_>>,
) -> Result<RequestBuilder, LxnsScoreError> {
    match body {
        Some(RequestBody::Player(player)) => Ok(request.json(player)),
        Some(RequestBody::Scores(scores)) => {
            #[derive(Serialize)]
            struct ScoresBody<'a> {
                scores: &'a [ScoreUpload],
            }
            Ok(request.json(&ScoresBody { scores }))
        }
        None => Ok(request),
    }
}

fn request_error(error: reqwest::Error) -> LxnsScoreError {
    if error.is_timeout() {
        LxnsScoreError::new(LxnsScoreErrorCode::Timeout, "LXNS score 请求超时")
    } else {
        LxnsScoreError::new(LxnsScoreErrorCode::Network, "LXNS score 网络请求失败")
    }
}
