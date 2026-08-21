mod auth;
mod response;
mod transport;

use std::fmt;

use reqwest::{Client, Url, header::HeaderMap, redirect::Policy};
use serde_json::json;

use super::{DivingFishRequest, ProviderError, ProviderErrorCode, QueryValue};
pub use response::DivingFishResponse;

const DEFAULT_API_BASE_URL: &str = "https://www.diving-fish.com/api/";
const DEFAULT_COVER_BASE_URL: &str = "https://www.diving-fish.com/covers/";

#[derive(Clone)]
pub struct DivingFishClient {
    http: Client,
    api_base: Url,
    cover_base: Url,
}

impl DivingFishClient {
    pub fn new() -> Result<Self, ProviderError> {
        let http = default_http_client()?;
        Self::with_http_client(http, DEFAULT_API_BASE_URL, DEFAULT_COVER_BASE_URL)
    }

    pub fn with_base_urls(api_base: &str, cover_base: &str) -> Result<Self, ProviderError> {
        let http = default_http_client()?;
        Self::with_http_client(http, api_base, cover_base)
    }

    pub fn with_http_client(
        http: Client,
        api_base: &str,
        cover_base: &str,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            http,
            api_base: parse_base_url(api_base, "API")?,
            cover_base: parse_base_url(cover_base, "封面")?,
        })
    }

    pub async fn execute(
        &self,
        request: DivingFishRequest,
    ) -> Result<DivingFishResponse, ProviderError> {
        auth::validate_request(&request)?;
        if !request.operation.metadata().http {
            return self.cover_response(&request);
        }

        let prepared = transport::prepare(&self.http, &self.api_base, &request)?;
        let response = prepared.builder.send().await.map_err(|error| {
            if error.is_timeout() {
                ProviderError::new(ProviderErrorCode::Timeout, "Diving-Fish 请求超时")
            } else {
                ProviderError::new(ProviderErrorCode::Network, "Diving-Fish 网络请求失败")
            }
        })?;
        response::decode(request.operation, response, &prepared.redactions).await
    }

    pub fn cover_url(&self, song_id: i64) -> Result<Url, ProviderError> {
        if song_id < 0 {
            return Err(ProviderError::new(
                ProviderErrorCode::InvalidRequest,
                "封面 song_id 必须是非负整数",
            ));
        }
        let cover_id = if 10_000 < song_id && song_id <= 11_000 {
            song_id - 10_000
        } else {
            song_id
        };
        self.cover_base
            .join(&format!("{cover_id:05}.png"))
            .map_err(|_| {
                ProviderError::new(
                    ProviderErrorCode::InvalidConfiguration,
                    "Diving-Fish 封面 URL 构造失败",
                )
            })
    }

    fn cover_response(
        &self,
        request: &DivingFishRequest,
    ) -> Result<DivingFishResponse, ProviderError> {
        let song_id = request
            .query
            .get("song_id")
            .or_else(|| request.query.get("id"))
            .and_then(single_query_value)
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or_else(|| {
                ProviderError::new(
                    ProviderErrorCode::InvalidRequest,
                    "maimai_cover_url 需要整数 query.song_id 或 query.id",
                )
            })?;
        let url = self.cover_url(song_id)?;
        Ok(DivingFishResponse::new(
            request.operation,
            200,
            url.clone(),
            HeaderMap::new(),
            Some(json!({"url": url.as_str()})),
            None,
            None,
        ))
    }
}

impl fmt::Debug for DivingFishClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DivingFishClient")
            .field("api_base", &self.api_base)
            .field("cover_base", &self.cover_base)
            .finish_non_exhaustive()
    }
}

fn default_http_client() -> Result<Client, ProviderError> {
    Client::builder()
        .user_agent("maimai-providers/0.1.0")
        .redirect(Policy::none())
        .build()
        .map_err(|_| {
            ProviderError::new(
                ProviderErrorCode::InvalidConfiguration,
                "Diving-Fish HTTP client 创建失败",
            )
        })
}

fn parse_base_url(value: &str, label: &str) -> Result<Url, ProviderError> {
    let mut url = Url::parse(value).map_err(|_| {
        ProviderError::new(
            ProviderErrorCode::InvalidConfiguration,
            format!("Diving-Fish {label} base URL 无效"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.cannot_be_a_base() {
        return Err(ProviderError::new(
            ProviderErrorCode::InvalidConfiguration,
            format!("Diving-Fish {label} base URL 必须是 HTTP(S) URL"),
        ));
    }
    url.set_query(None);
    url.set_fragment(None);
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn single_query_value(value: &QueryValue) -> Option<&str> {
    match value {
        QueryValue::Single(value) => Some(value),
        QueryValue::Multiple(values) if values.len() == 1 => values.first().map(String::as_str),
        QueryValue::Multiple(_) => None,
    }
}
