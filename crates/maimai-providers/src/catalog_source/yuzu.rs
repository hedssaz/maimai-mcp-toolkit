use super::CatalogSourceClient;
use super::decode::{DecodedDocument, invalid_shape};
use super::error::{CatalogSourceError, EndpointId};
use super::request::RequestOptions;
use super::types::{CatalogSource, DocumentStatistics, SourceBundle, SourceMetadata, SourceTarget};

pub(crate) async fn fetch_aliases(
    client: &CatalogSourceClient,
) -> Result<SourceBundle, CatalogSourceError> {
    let response = client
        .request(EndpointId::YuzuAliases, RequestOptions::default())
        .await?
        .require_updated(CatalogSource::Yuzu, EndpointId::YuzuAliases)?;
    let document = DecodedDocument::parse(response, CatalogSource::Yuzu, EndpointId::YuzuAliases)?;
    let records = yuzu_alias_count(document.value())
        .ok_or_else(|| invalid_shape(CatalogSource::Yuzu, EndpointId::YuzuAliases))?;
    let document = document.into_document(
        SourceTarget::YuzuAliasList,
        DocumentStatistics::Records { records },
    );
    Ok(SourceBundle::updated(
        CatalogSource::Yuzu,
        vec![document],
        None,
        SourceMetadata::empty(),
    ))
}

fn yuzu_alias_count(value: &serde_json::Value) -> Option<usize> {
    if let Some(aliases) = value.as_array() {
        return Some(aliases.len());
    }
    let object = value.as_object()?;
    match object.get("content") {
        Some(content) => content.as_array().map(Vec::len),
        None => object
            .get("aliases")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
    }
}

pub(crate) async fn fetch_plate(
    client: &CatalogSourceClient,
) -> Result<SourceBundle, CatalogSourceError> {
    let response = client
        .request(EndpointId::YuzuPlate, RequestOptions::default())
        .await?
        .require_updated(CatalogSource::Plate, EndpointId::YuzuPlate)?;
    let document = DecodedDocument::parse(response, CatalogSource::Plate, EndpointId::YuzuPlate)?;
    let plates = {
        let root = document
            .value()
            .as_object()
            .ok_or_else(|| invalid_shape(CatalogSource::Plate, EndpointId::YuzuPlate))?;
        let content_value = root.get("content").unwrap_or(document.value());
        let content = content_value
            .as_object()
            .ok_or_else(|| invalid_shape(CatalogSource::Plate, EndpointId::YuzuPlate))?;
        if content.values().any(|songs| !songs.is_array()) {
            return Err(invalid_shape(CatalogSource::Plate, EndpointId::YuzuPlate));
        }
        content.len()
    };
    let document =
        document.into_document(SourceTarget::Plate, DocumentStatistics::Plates { plates });
    Ok(SourceBundle::updated(
        CatalogSource::Plate,
        vec![document],
        None,
        SourceMetadata::empty(),
    ))
}
