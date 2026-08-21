use maimai_providers::{
    BundleStatus, CatalogSource, CatalogSourceError, EntityTag, SourceBundle, SourceTarget,
};

#[derive(Clone, Debug)]
pub(crate) struct FetchFailure {
    pub(crate) code: String,
    pub(crate) message: String,
}

impl From<CatalogSourceError> for FetchFailure {
    fn from(error: CatalogSourceError) -> Self {
        Self {
            code: error.code().to_string(),
            message: error.safe_message().to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FetchedDocument {
    pub(crate) target: SourceTarget,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FetchedStatus {
    Updated,
    NotModified,
}

#[derive(Clone, Debug)]
pub(crate) struct FetchedBundle {
    pub(crate) source: CatalogSource,
    pub(crate) status: FetchedStatus,
    pub(crate) documents: Vec<FetchedDocument>,
    pub(crate) etag: Option<EntityTag>,
}

impl From<SourceBundle> for FetchedBundle {
    fn from(bundle: SourceBundle) -> Self {
        let source = bundle.source();
        let status = match bundle.status() {
            BundleStatus::Updated => FetchedStatus::Updated,
            BundleStatus::NotModified => FetchedStatus::NotModified,
        };
        let etag = bundle.etag().cloned();
        let documents = bundle
            .into_documents()
            .into_iter()
            .map(|document| FetchedDocument {
                target: document.target(),
                bytes: document.into_bytes(),
            })
            .collect();
        Self {
            source,
            status,
            documents,
            etag,
        }
    }
}
