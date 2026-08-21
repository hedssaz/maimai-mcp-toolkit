use reqwest::{
    Client, RequestBuilder, Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderValue, IF_NONE_MATCH},
};
use secrecy::ExposeSecret;
use serde::Serialize;

use super::super::redaction::secret_redactions;
use super::super::{DivingFishOperation, DivingFishRequest, ProviderError, ProviderErrorCode};
use super::auth::{apply_headers, auth_required};

pub(super) struct PreparedRequest {
    pub(super) builder: RequestBuilder,
    pub(super) redactions: Vec<String>,
}

pub(super) fn prepare(
    http: &Client,
    api_base: &Url,
    request: &DivingFishRequest,
) -> Result<PreparedRequest, ProviderError> {
    let url = operation_url(api_base, request)?;
    let redactions = secret_redactions(request);
    let mut headers = request.headers.clone();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    apply_headers(&mut headers, &request.credentials)?;
    if let Some(etag) = request.if_none_match.clone() {
        headers.insert(IF_NONE_MATCH, etag);
    }

    let metadata = request.operation.metadata();
    let mut builder = http
        .request(metadata.method.as_reqwest(), url)
        .headers(headers)
        .timeout(request.timeout);
    if request.operation == DivingFishOperation::MaimaiLogin {
        let username = request
            .credentials
            .login_username
            .as_deref()
            .ok_or_else(auth_required)?;
        let password = request
            .credentials
            .login_password
            .as_ref()
            .ok_or_else(auth_required)?;
        #[derive(Serialize)]
        struct LoginBody<'a> {
            username: &'a str,
            password: &'a str,
        }
        builder = builder.json(&LoginBody {
            username,
            password: password.expose_secret(),
        });
    } else if let Some(raw_body) = request.raw_body.as_ref() {
        if !request.headers.contains_key(CONTENT_TYPE) {
            builder = builder.header(CONTENT_TYPE, "text/html; charset=utf-8");
        }
        builder = builder.body(raw_body.clone());
    } else if let Some(body) = request.body.as_ref() {
        builder = builder.json(body);
    }
    Ok(PreparedRequest {
        builder,
        redactions,
    })
}

fn operation_url(api_base: &Url, request: &DivingFishRequest) -> Result<Url, ProviderError> {
    let metadata = request.operation.metadata();
    let relative = format!(
        "{}{path}",
        metadata.game.path_segment(),
        path = metadata.path
    );
    let mut url = api_base.join(&relative).map_err(|_| {
        ProviderError::new(
            ProviderErrorCode::InvalidConfiguration,
            "Diving-Fish endpoint URL 构造失败",
        )
    })?;
    if !request.query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in &request.query {
            match value {
                super::super::QueryValue::Single(value) => {
                    pairs.append_pair(key, value.as_str());
                }
                super::super::QueryValue::Multiple(values) => {
                    for value in values {
                        pairs.append_pair(key, value.as_str());
                    }
                }
            }
        }
    }
    Ok(url)
}
