use serde_json::Value;

use super::CatalogSourceClient;
use super::decode::{DecodedDocument, invalid_shape};
use super::error::{CatalogSourceError, EndpointId};
use super::request::RequestOptions;
use super::types::{CatalogSource, DocumentStatistics, SourceBundle, SourceMetadata, SourceTarget};

const CONTAINER_KEYS: [&str; 7] = [
    "content",
    "data",
    "locations",
    "locationList",
    "shops",
    "items",
    "result",
];
const LOCATION_KEYS: [&str; 6] = ["id", "placeId", "shopId", "name", "shopName", "address"];

pub(crate) async fn fetch(
    client: &CatalogSourceClient,
) -> Result<SourceBundle, CatalogSourceError> {
    let response = client
        .request(EndpointId::WahlapLocations, RequestOptions::default())
        .await?
        .require_updated(CatalogSource::Location, EndpointId::WahlapLocations)?;
    let document = DecodedDocument::parse(
        response,
        CatalogSource::Location,
        EndpointId::WahlapLocations,
    )?;
    let locations = location_count(document.value());
    if locations == 0 {
        return Err(invalid_shape(
            CatalogSource::Location,
            EndpointId::WahlapLocations,
        ));
    }
    let document = document.into_document(
        SourceTarget::Location,
        DocumentStatistics::Locations { locations },
    );
    Ok(SourceBundle::updated(
        CatalogSource::Location,
        vec![document],
        None,
        SourceMetadata::empty(),
    ))
}

fn location_count(value: &Value) -> usize {
    if let Some(items) = value.as_array() {
        return items.iter().filter(|item| item.is_object()).count();
    }
    let Some(object) = value.as_object() else {
        return 0;
    };
    for key in CONTAINER_KEYS {
        let count = object.get(key).map(location_count).unwrap_or(0);
        if count > 0 {
            return count;
        }
    }
    usize::from(LOCATION_KEYS.iter().any(|key| object.contains_key(*key)))
}
