use serde_json::Value;
use sha2::{Digest, Sha256};

use super::error::{CatalogSourceError, CatalogSourceErrorCode, EndpointId};
use super::request::HttpResponse;
use super::types::{
    CatalogSource, DocumentDigest, DocumentStatistics, SourceDocument, SourceTarget,
};

pub(crate) struct DecodedDocument {
    value: Value,
    bytes: Vec<u8>,
    content_type: Option<String>,
}

impl DecodedDocument {
    pub(crate) fn parse(
        response: HttpResponse,
        source: CatalogSource,
        endpoint: EndpointId,
    ) -> Result<Self, CatalogSourceError> {
        let value = serde_json::from_slice(&response.bytes).map_err(|_| {
            CatalogSourceError::response(
                CatalogSourceErrorCode::InvalidJson,
                source,
                endpoint,
                None,
                "source response is not valid JSON",
            )
        })?;
        Ok(Self {
            value,
            bytes: response.bytes,
            content_type: response.content_type,
        })
    }

    pub(crate) const fn value(&self) -> &Value {
        &self.value
    }

    pub(crate) fn into_document(
        self,
        target: SourceTarget,
        statistics: DocumentStatistics,
    ) -> SourceDocument {
        let digest = Sha256::digest(&self.bytes);
        SourceDocument::new(
            target,
            self.bytes,
            DocumentDigest::new(digest.into()),
            statistics,
            self.content_type,
        )
    }
}

pub(crate) fn invalid_shape(source: CatalogSource, endpoint: EndpointId) -> CatalogSourceError {
    CatalogSourceError::response(
        CatalogSourceErrorCode::InvalidShape,
        source,
        endpoint,
        None,
        "source JSON does not match the required document shape",
    )
}

pub(crate) fn envelope_list_len(value: &Value, keys: &[&str]) -> Option<usize> {
    if let Some(items) = value.as_array() {
        return Some(items.len());
    }
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_array).map(Vec::len))
}
