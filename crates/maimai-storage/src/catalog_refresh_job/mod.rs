mod decode;
mod lifecycle;
mod model;
mod owner;
mod schema;
mod store;

use std::path::Path;

use sqlx::SqlitePool;

use crate::{
    StorageError,
    store::{harden_sqlite_files, open_sqlite_pool},
};

pub use model::{
    CatalogRefreshJob, CatalogRefreshJobId, CatalogRefreshJobOutcome, CatalogRefreshJobSource,
    CatalogRefreshJobStart, CatalogRefreshJobStatus, CatalogRefreshSourceStatus,
    CatalogRefreshSourceUpdate, CatalogRefreshTerminalUpdate,
};
pub use owner::{CatalogRefreshOwner, CatalogRefreshOwnerClaim};

#[derive(Clone, Debug)]
pub struct CatalogRefreshJobStore {
    pool: SqlitePool,
    database_path: std::path::PathBuf,
}

impl CatalogRefreshJobStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let pool = open_sqlite_pool(path).await?;
        schema::initialize(&pool).await?;
        harden_sqlite_files(path)?;
        let database_path = path
            .canonicalize()
            .map_err(|source| StorageError::PreparePath {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self {
            pool,
            database_path,
        })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests;
