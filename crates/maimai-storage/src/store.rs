use std::{path::Path, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::{LegacyImportReport, StorageError, schema};

pub(crate) mod codec;
mod path;
mod preference;
mod profile;
mod record;
mod snapshot;

const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct StateStore {
    pub(crate) pool: SqlitePool,
    legacy_import_report: LegacyImportReport,
}

impl StateStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let pool = open_sqlite_pool(path).await?;
        let legacy_import_report = schema::initialize(&pool).await?;
        harden_sqlite_files(path)?;
        Ok(Self {
            pool,
            legacy_import_report,
        })
    }

    pub fn legacy_import_report(&self) -> &LegacyImportReport {
        &self.legacy_import_report
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

pub(crate) async fn open_sqlite_pool(path: &Path) -> Result<SqlitePool, StorageError> {
    path::prepare_database_path(path)?;
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    Ok(pool)
}

pub(crate) fn harden_sqlite_files(path: &Path) -> Result<(), StorageError> {
    path::harden_database_files(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use tempfile::TempDir;

    use super::StateStore;

    #[tokio::test]
    async fn every_pool_connection_uses_required_sqlite_pragmas()
    -> Result<(), Box<dyn Error + Send + Sync>> {
        let temp = TempDir::new()?;
        let store = StateStore::open(temp.path().join("state.db")).await?;

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&store.pool)
            .await?;
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&store.pool)
            .await?;
        let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&store.pool)
            .await?;

        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        assert_eq!(foreign_keys, 1);
        assert_eq!(busy_timeout, 30_000);
        store.close().await;
        Ok(())
    }
}
