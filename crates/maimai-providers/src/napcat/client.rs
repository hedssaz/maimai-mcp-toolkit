use std::fmt;

use reqwest::{Client, redirect::Policy};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use super::{
    Friend, Group, GroupMember, MAX_RESPONSE_BODY_BYTES, NapCatConfig, NapCatError,
    NapCatErrorCode, OneBotEnvelope,
    redaction::{sanitize_error_body, sanitize_text},
};

#[derive(Clone)]
pub struct NapCatClient {
    http: Client,
    config: NapCatConfig,
}

impl NapCatClient {
    pub fn new(config: NapCatConfig) -> Result<Self, NapCatError> {
        let http = Client::builder()
            .user_agent("maimai-providers/0.1.0")
            .redirect(Policy::none())
            .build()
            .map_err(|_| {
                NapCatError::new(
                    NapCatErrorCode::InvalidConfiguration,
                    "NapCat HTTP client 创建失败",
                )
            })?;
        Ok(Self { http, config })
    }

    pub fn config(&self) -> &NapCatConfig {
        &self.config
    }

    pub async fn get_friend_list(&self) -> Result<OneBotEnvelope<Vec<Friend>>, NapCatError> {
        self.post_list("get_friend_list", &EmptyRequest {}).await
    }

    pub async fn get_group_list(&self) -> Result<OneBotEnvelope<Vec<Group>>, NapCatError> {
        self.post_list("get_group_list", &EmptyRequest {}).await
    }

    pub async fn get_group_member_list(
        &self,
        group_id: &str,
        no_cache: bool,
    ) -> Result<OneBotEnvelope<Vec<GroupMember>>, NapCatError> {
        let group_id = normalize_request_group_id(group_id)?;
        let request = GroupMemberListRequest {
            group_id: GroupIdRequest::from(group_id.as_str()),
            no_cache,
        };
        let mut response = self
            .post_list::<GroupMember, _>("get_group_member_list", &request)
            .await?;
        for member in &mut response.data {
            match member.group_id.as_deref() {
                Some(actual) if actual != group_id => {
                    return Err(NapCatError::invalid_response(None));
                }
                Some(_) => {}
                None => member.group_id = Some(group_id.clone()),
            }
        }
        Ok(response)
    }

    async fn post_list<T, B>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<OneBotEnvelope<Vec<T>>, NapCatError>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        let url = self.config.endpoint(endpoint)?;
        let access_token = self.config.access_token().map(ExposeSecret::expose_secret);
        let mut request = self
            .http
            .post(url)
            .timeout(self.config.timeout())
            .json(body);
        if let Some(access_token) = access_token {
            request = request.bearer_auth(access_token);
        }

        let mut response = request.send().await.map_err(request_error)?;
        let http_status = response.status();
        let bytes = read_body(&mut response).await?;
        let secrets = [access_token.unwrap_or_default()];
        let text = String::from_utf8_lossy(&bytes);

        if !http_status.is_success() {
            let body = (!text.is_empty()).then(|| sanitize_error_body(&text, &secrets));
            return Err(NapCatError::http(http_status.as_u16(), body));
        }

        parse_list_response(&bytes, &secrets)
    }
}

async fn read_body(response: &mut reqwest::Response) -> Result<Vec<u8>, NapCatError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
    {
        return Err(NapCatError::response_too_large());
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
            .ok_or_else(NapCatError::response_too_large)?;
        if next_len > MAX_RESPONSE_BODY_BYTES {
            return Err(NapCatError::response_too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

impl fmt::Debug for NapCatClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NapCatClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct EmptyRequest {}

#[derive(Serialize)]
struct GroupMemberListRequest<'a> {
    group_id: GroupIdRequest<'a>,
    no_cache: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
enum GroupIdRequest<'a> {
    Number(u64),
    Text(&'a str),
}

impl<'a> From<&'a str> for GroupIdRequest<'a> {
    fn from(value: &'a str) -> Self {
        value.parse::<u64>().map_or(Self::Text(value), Self::Number)
    }
}

#[derive(Deserialize)]
struct EnvelopeWire {
    status: Option<String>,
    retcode: Option<i64>,
    data: Option<Value>,
    message: Option<String>,
    wording: Option<String>,
}

fn parse_list_response<T>(
    bytes: &[u8],
    secrets: &[&str],
) -> Result<OneBotEnvelope<Vec<T>>, NapCatError>
where
    T: DeserializeOwned,
{
    let raw_text = String::from_utf8_lossy(bytes);
    let value = serde_json::from_slice::<Value>(bytes).map_err(|_| {
        NapCatError::invalid_response(
            (!raw_text.is_empty()).then(|| sanitize_error_body(&raw_text, secrets)),
        )
    })?;

    if value.is_array() {
        let data = serde_json::from_value::<Vec<T>>(value).map_err(|_| {
            NapCatError::invalid_response(Some(sanitize_error_body(&raw_text, secrets)))
        })?;
        return Ok(OneBotEnvelope::new(None, None, None, data));
    }

    let wire = serde_json::from_value::<EnvelopeWire>(value).map_err(|_| {
        NapCatError::invalid_response(Some(sanitize_error_body(&raw_text, secrets)))
    })?;
    let status = wire
        .status
        .map(|value| sanitize_text(value.trim(), secrets))
        .filter(|value| !value.is_empty());
    let provider_message = wire
        .message
        .filter(|value| !value.trim().is_empty())
        .or_else(|| wire.wording.filter(|value| !value.trim().is_empty()))
        .map(|value| sanitize_text(value.trim(), secrets));
    let rejected_status = status
        .as_deref()
        .is_some_and(|status| !status.eq_ignore_ascii_case("ok"));
    let rejected_retcode = wire.retcode.is_some_and(|retcode| retcode != 0);
    if rejected_status || rejected_retcode {
        return Err(NapCatError::onebot(
            status,
            wire.retcode,
            provider_message,
            Some(sanitize_error_body(&raw_text, secrets)),
        ));
    }

    let data = wire.data.ok_or_else(|| {
        NapCatError::invalid_response(Some(sanitize_error_body(&raw_text, secrets)))
    })?;
    let data = serde_json::from_value::<Vec<T>>(data).map_err(|_| {
        NapCatError::invalid_response(Some(sanitize_error_body(&raw_text, secrets)))
    })?;
    Ok(OneBotEnvelope::new(
        status,
        wire.retcode,
        provider_message,
        data,
    ))
}

fn normalize_request_group_id(value: &str) -> Result<String, NapCatError> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(NapCatError::new(
            NapCatErrorCode::InvalidRequest,
            "NapCat group_id 格式不正确",
        ));
    }
    Ok(value.to_owned())
}

fn request_error(error: reqwest::Error) -> NapCatError {
    if error.is_timeout() {
        NapCatError::new(NapCatErrorCode::Timeout, "NapCat 请求超时")
    } else {
        NapCatError::new(NapCatErrorCode::Network, "NapCat 网络请求失败")
    }
}
