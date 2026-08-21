use super::CatalogSourceClient;
use super::decode::{DecodedDocument, invalid_shape};
use super::error::{CatalogSourceError, CatalogSourceErrorCode, EndpointId};
use super::request::RequestOptions;
use super::types::{CatalogSource, DocumentStatistics, SourceBundle, SourceMetadata, SourceTarget};

pub(crate) async fn fetch(
    client: &CatalogSourceClient,
) -> Result<SourceBundle, CatalogSourceError> {
    let (payload, endpoint) = match client
        .request(EndpointId::DxDataProxy, RequestOptions::default())
        .await
    {
        Ok(payload) => (payload, EndpointId::DxDataProxy),
        Err(_) => (
            client
                .request(EndpointId::DxDataGithub, RequestOptions::default())
                .await
                .map_err(|_| {
                    CatalogSourceError::source(
                        CatalogSourceErrorCode::AllEndpointsFailed,
                        CatalogSource::DxData,
                        "all configured dxdata endpoints failed",
                    )
                })?,
            EndpointId::DxDataGithub,
        ),
    };
    let response = payload.require_updated(CatalogSource::DxData, endpoint)?;
    decode(response, endpoint)
}

fn decode(
    response: super::request::HttpResponse,
    endpoint: EndpointId,
) -> Result<SourceBundle, CatalogSourceError> {
    let document = DecodedDocument::parse(response, CatalogSource::DxData, endpoint)?;
    let object = document
        .value()
        .as_object()
        .ok_or_else(|| invalid_shape(CatalogSource::DxData, endpoint))?;
    let records = object
        .get("songs")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| invalid_shape(CatalogSource::DxData, endpoint))?;
    let update_time = object
        .get("updateTime")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let document = document.into_document(
        SourceTarget::DxData,
        DocumentStatistics::Records { records },
    );
    Ok(SourceBundle::updated(
        CatalogSource::DxData,
        vec![document],
        None,
        SourceMetadata::with_update_time(update_time),
    ))
}
