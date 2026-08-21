use thiserror::Error;

#[derive(Debug, Error)]
pub(super) enum CatalogToolError {
    #[error("{0}")]
    Input(String),

    #[error(transparent)]
    Query(#[from] maimai_catalog::QueryError),

    #[error(transparent)]
    Catalog(#[from] maimai_catalog::CatalogError),

    #[error(transparent)]
    Alias(#[from] maimai_catalog::AliasError),

    #[error(transparent)]
    Store(#[from] maimai_catalog::CatalogStoreError),

    #[error(transparent)]
    Refresh(#[from] maimai_app::catalog_refresh::RefreshError),

    #[error(transparent)]
    RefreshJob(#[from] maimai_app::catalog_refresh::job::RefreshJobError),

    #[error(transparent)]
    Today(#[from] maimai_core::TodayError),

    #[error("failed to serialize catalog result: {0}")]
    Json(#[from] serde_json::Error),

    #[error("failed to format refresh job timestamp: {0}")]
    Timestamp(#[from] time::error::Format),
}

impl CatalogToolError {
    pub(super) fn input(message: impl Into<String>) -> Self {
        Self::Input(message.into())
    }
}
