use serde_json::Value;

use super::CatalogSourceClient;
use super::decode::{DecodedDocument, invalid_shape};
use super::error::{CatalogSourceError, EndpointId};
use super::request::RequestOptions;
use super::types::{CatalogSource, DocumentStatistics, SourceBundle, SourceMetadata, SourceTarget};

pub(crate) async fn fetch_aliases(
    client: &CatalogSourceClient,
) -> Result<SourceBundle, CatalogSourceError> {
    let response = client
        .request(EndpointId::DxRatingAliases, RequestOptions::default())
        .await?
        .require_updated(CatalogSource::DxRatingAliases, EndpointId::DxRatingAliases)?;
    let document = DecodedDocument::parse(
        response,
        CatalogSource::DxRatingAliases,
        EndpointId::DxRatingAliases,
    )?;
    let records = document.value().as_array().map(Vec::len).ok_or_else(|| {
        invalid_shape(CatalogSource::DxRatingAliases, EndpointId::DxRatingAliases)
    })?;
    let document = document.into_document(
        SourceTarget::DxRatingAliases,
        DocumentStatistics::Records { records },
    );
    Ok(SourceBundle::updated(
        CatalogSource::DxRatingAliases,
        vec![document],
        None,
        SourceMetadata::empty(),
    ))
}

pub(crate) async fn fetch_tags(
    client: &CatalogSourceClient,
) -> Result<SourceBundle, CatalogSourceError> {
    let response = client
        .request(EndpointId::DxRatingTags, RequestOptions::default())
        .await?
        .require_updated(CatalogSource::DxRatingTags, EndpointId::DxRatingTags)?;
    let document = DecodedDocument::parse(
        response,
        CatalogSource::DxRatingTags,
        EndpointId::DxRatingTags,
    )?;
    let object = document
        .value()
        .as_object()
        .ok_or_else(|| invalid_shape(CatalogSource::DxRatingTags, EndpointId::DxRatingTags))?;
    if !object.contains_key("tags") {
        return Err(invalid_shape(
            CatalogSource::DxRatingTags,
            EndpointId::DxRatingTags,
        ));
    }
    let tags = optional_array_len(object.get("tags"))
        .ok_or_else(|| invalid_shape(CatalogSource::DxRatingTags, EndpointId::DxRatingTags))?;
    let groups = optional_array_len(object.get("tagGroups"))
        .ok_or_else(|| invalid_shape(CatalogSource::DxRatingTags, EndpointId::DxRatingTags))?;
    let associations = optional_array_len(object.get("tagSongs"))
        .ok_or_else(|| invalid_shape(CatalogSource::DxRatingTags, EndpointId::DxRatingTags))?;
    let document = document.into_document(
        SourceTarget::DxRatingTags,
        DocumentStatistics::Tags {
            tags,
            groups,
            associations,
        },
    );
    Ok(SourceBundle::updated(
        CatalogSource::DxRatingTags,
        vec![document],
        None,
        SourceMetadata::empty(),
    ))
}

fn optional_array_len(value: Option<&Value>) -> Option<usize> {
    match value {
        None | Some(Value::Null) => Some(0),
        Some(Value::Array(values)) => Some(values.len()),
        Some(_) => None,
    }
}
