use reqwest::header::{CONTENT_TYPE, ETAG, IF_NONE_MATCH, USER_AGENT};

use super::CatalogSourceClient;
use super::error::{CatalogSourceError, CatalogSourceErrorCode, EndpointId};
use super::types::EntityTag;

pub(crate) enum HttpPayload {
    Updated(HttpResponse),
    NotModified(Option<EntityTag>),
}

impl HttpPayload {
    pub(crate) fn require_updated(
        self,
        source: super::types::CatalogSource,
        endpoint: EndpointId,
    ) -> Result<HttpResponse, CatalogSourceError> {
        match self {
            Self::Updated(response) => Ok(response),
            Self::NotModified(_) => Err(CatalogSourceError::response(
                CatalogSourceErrorCode::HttpStatus,
                source,
                endpoint,
                Some(304),
                "source returned not-modified without conditional support",
            )),
        }
    }
}

pub(crate) struct HttpResponse {
    pub(crate) bytes: Vec<u8>,
    pub(crate) etag: Option<EntityTag>,
    pub(crate) content_type: Option<String>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct RequestOptions<'a> {
    pub(crate) if_none_match: Option<&'a EntityTag>,
    pub(crate) lxns_notes: bool,
}

impl CatalogSourceClient {
    pub(crate) async fn request(
        &self,
        endpoint_id: EndpointId,
        options: RequestOptions<'_>,
    ) -> Result<HttpPayload, CatalogSourceError> {
        let endpoint = self.endpoints.get(endpoint_id)?;
        let mut url = endpoint.url.clone();
        if options.lxns_notes {
            url.query_pairs_mut().append_pair("notes", "true");
        }
        let timeout = self.config.timeout_override().unwrap_or(endpoint.timeout);
        let user_agent = endpoint
            .user_agent
            .unwrap_or_else(|| self.config.user_agent());
        let mut request = self
            .http
            .get(url)
            .timeout(timeout)
            .header(USER_AGENT, user_agent);
        if let Some(etag) = options.if_none_match {
            request = request.header(IF_NONE_MATCH, etag.as_header_value());
        }

        let mut response = request.send().await.map_err(|error| {
            let code = if error.is_timeout() {
                CatalogSourceErrorCode::Timeout
            } else {
                CatalogSourceErrorCode::Network
            };
            CatalogSourceError::response(
                code,
                endpoint.source,
                endpoint.id,
                None,
                if error.is_timeout() {
                    "source request timed out"
                } else {
                    "source request failed"
                },
            )
        })?;

        let status = response.status();
        let response_etag = parse_etag(&response, endpoint.source, endpoint.id)?;
        if status.as_u16() == 304 {
            return Ok(HttpPayload::NotModified(response_etag));
        }
        if status.is_redirection() {
            return Err(CatalogSourceError::response(
                CatalogSourceErrorCode::Redirect,
                endpoint.source,
                endpoint.id,
                Some(status.as_u16()),
                "source endpoint returned a redirect",
            ));
        }
        if !status.is_success() {
            return Err(CatalogSourceError::response(
                CatalogSourceErrorCode::HttpStatus,
                endpoint.source,
                endpoint.id,
                Some(status.as_u16()),
                "source endpoint returned an unsuccessful status",
            ));
        }

        let content_type = parse_content_type(&response, endpoint.source, endpoint.id)?;
        if let Some(value) = content_type.as_deref()
            && !content_type_allowed(value, endpoint.accepts_text)
        {
            return Err(CatalogSourceError::response(
                CatalogSourceErrorCode::InvalidContentType,
                endpoint.source,
                endpoint.id,
                Some(status.as_u16()),
                "source response content type is not JSON-compatible",
            ));
        }
        if response.content_length().is_some_and(|length| {
            length > u64::try_from(self.config.max_body_bytes()).unwrap_or(u64::MAX)
        }) {
            return Err(body_too_large(endpoint.source, endpoint.id));
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            CatalogSourceError::response(
                if error.is_timeout() {
                    CatalogSourceErrorCode::Timeout
                } else {
                    CatalogSourceErrorCode::Network
                },
                endpoint.source,
                endpoint.id,
                Some(status.as_u16()),
                "source response body could not be read",
            )
        })? {
            let next_length = bytes
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| body_too_large(endpoint.source, endpoint.id))?;
            if next_length > self.config.max_body_bytes() {
                return Err(body_too_large(endpoint.source, endpoint.id));
            }
            bytes.extend_from_slice(&chunk);
        }

        Ok(HttpPayload::Updated(HttpResponse {
            bytes,
            etag: response_etag,
            content_type,
        }))
    }
}

fn parse_etag(
    response: &reqwest::Response,
    source: super::types::CatalogSource,
    endpoint: EndpointId,
) -> Result<Option<EntityTag>, CatalogSourceError> {
    response
        .headers()
        .get(ETAG)
        .map(|value| {
            let text = value.to_str().map_err(|_| {
                CatalogSourceError::response(
                    CatalogSourceErrorCode::InvalidResponseHeader,
                    source,
                    endpoint,
                    Some(response.status().as_u16()),
                    "source response contains an invalid entity tag",
                )
            })?;
            EntityTag::parse(text).map_err(|_| {
                CatalogSourceError::response(
                    CatalogSourceErrorCode::InvalidResponseHeader,
                    source,
                    endpoint,
                    Some(response.status().as_u16()),
                    "source response contains an invalid entity tag",
                )
            })
        })
        .transpose()
}

fn parse_content_type(
    response: &reqwest::Response,
    source: super::types::CatalogSource,
    endpoint: EndpointId,
) -> Result<Option<String>, CatalogSourceError> {
    response
        .headers()
        .get(CONTENT_TYPE)
        .map(|value| {
            value.to_str().map(str::to_owned).map_err(|_| {
                CatalogSourceError::response(
                    CatalogSourceErrorCode::InvalidResponseHeader,
                    source,
                    endpoint,
                    Some(response.status().as_u16()),
                    "source response contains an invalid content type",
                )
            })
        })
        .transpose()
}

fn content_type_allowed(value: &str, accepts_text: bool) -> bool {
    let media_type = value
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    media_type == "application/json"
        || media_type.ends_with("+json")
        || (accepts_text && media_type == "text/plain")
}

fn body_too_large(source: super::types::CatalogSource, endpoint: EndpointId) -> CatalogSourceError {
    CatalogSourceError::response(
        CatalogSourceErrorCode::BodyTooLarge,
        source,
        endpoint,
        None,
        "source response exceeds the configured body size limit",
    )
}
