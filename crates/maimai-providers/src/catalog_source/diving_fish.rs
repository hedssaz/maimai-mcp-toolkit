use super::CatalogSourceClient;
use super::decode::{DecodedDocument, invalid_shape};
use super::error::{CatalogSourceError, EndpointId};
use super::request::{HttpPayload, RequestOptions};
use super::types::{CatalogSource, DocumentStatistics, SourceBundle, SourceMetadata, SourceTarget};

pub(crate) async fn fetch_music(
    client: &CatalogSourceClient,
    etag: Option<&super::types::EntityTag>,
) -> Result<SourceBundle, CatalogSourceError> {
    let payload = client
        .request(
            EndpointId::DivingFishMusic,
            RequestOptions {
                if_none_match: etag,
                lxns_notes: false,
            },
        )
        .await?;
    let response = match payload {
        HttpPayload::NotModified(response_etag) => {
            return Ok(SourceBundle::not_modified(
                CatalogSource::DivingFish,
                response_etag.or_else(|| etag.cloned()),
            ));
        }
        HttpPayload::Updated(response) => response,
    };
    let etag = response.etag.clone();
    let document = DecodedDocument::parse(
        response,
        CatalogSource::DivingFish,
        EndpointId::DivingFishMusic,
    )?;
    let records = document
        .value()
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| invalid_shape(CatalogSource::DivingFish, EndpointId::DivingFishMusic))?;
    let document = document.into_document(
        SourceTarget::DivingFishSongList,
        DocumentStatistics::Records { records },
    );
    Ok(SourceBundle::updated(
        CatalogSource::DivingFish,
        vec![document],
        etag,
        SourceMetadata::empty(),
    ))
}

pub(crate) async fn fetch_stats(
    client: &CatalogSourceClient,
) -> Result<SourceBundle, CatalogSourceError> {
    let response = client
        .request(EndpointId::DivingFishChartStats, RequestOptions::default())
        .await?
        .require_updated(CatalogSource::ChartStats, EndpointId::DivingFishChartStats)?;
    let document = DecodedDocument::parse(
        response,
        CatalogSource::ChartStats,
        EndpointId::DivingFishChartStats,
    )?;
    let object = document.value().as_object().ok_or_else(|| {
        invalid_shape(CatalogSource::ChartStats, EndpointId::DivingFishChartStats)
    })?;
    let charts = object
        .get("charts")
        .and_then(serde_json::Value::as_object)
        .map(serde_json::Map::len)
        .ok_or_else(|| {
            invalid_shape(CatalogSource::ChartStats, EndpointId::DivingFishChartStats)
        })?;
    let difficulty_buckets = match object.get("diff_data") {
        None | Some(serde_json::Value::Null) => 0,
        Some(serde_json::Value::Object(values)) => values.len(),
        Some(_) => {
            return Err(invalid_shape(
                CatalogSource::ChartStats,
                EndpointId::DivingFishChartStats,
            ));
        }
    };
    let document = document.into_document(
        SourceTarget::DivingFishChartStats,
        DocumentStatistics::ChartStats {
            charts,
            difficulty_buckets,
        },
    );
    Ok(SourceBundle::updated(
        CatalogSource::ChartStats,
        vec![document],
        None,
        SourceMetadata::empty(),
    ))
}
