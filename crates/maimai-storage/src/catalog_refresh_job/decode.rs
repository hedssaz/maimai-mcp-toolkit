use std::collections::HashSet;

use sqlx::{Row, sqlite::SqliteRow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    CatalogRefreshJob, CatalogRefreshJobId, CatalogRefreshJobOutcome, CatalogRefreshJobSource,
    CatalogRefreshJobStatus, CatalogRefreshSourceStatus,
};
use crate::StorageError;

pub(super) fn job_from_row(
    row: SqliteRow,
    sources: Vec<CatalogRefreshJobSource>,
) -> Result<CatalogRefreshJob, StorageError> {
    let status: String = row.try_get("status")?;
    let outcome: Option<String> = row.try_get("outcome")?;
    Ok(CatalogRefreshJob {
        id: CatalogRefreshJobId::from_stored(row.try_get("id")?)?,
        status: CatalogRefreshJobStatus::from_stored(&status)?,
        outcome: outcome
            .as_deref()
            .map(CatalogRefreshJobOutcome::from_stored)
            .transpose()?,
        created_at: timestamp(&row, "created_at")?,
        started_at: optional_timestamp(&row, "started_at")?,
        finished_at: optional_timestamp(&row, "finished_at")?,
        message: row.try_get("message")?,
        error_code: row.try_get("error_code")?,
        error_message: row.try_get("error_message")?,
        sources,
    })
}

pub(super) fn source_from_row(row: SqliteRow) -> Result<CatalogRefreshJobSource, StorageError> {
    let status: String = row.try_get("status")?;
    let duration: Option<i64> = row.try_get("duration_millis")?;
    Ok(CatalogRefreshJobSource {
        position: u32::try_from(row.try_get::<i64, _>("position")?)
            .map_err(|_| invalid("catalog_refresh_job_source.position", "out of range"))?,
        source: row.try_get("source")?,
        due: row.try_get::<i64, _>("due")? != 0,
        status: CatalogRefreshSourceStatus::from_stored(&status)?,
        duration_millis: duration
            .map(|value| {
                u64::try_from(value)
                    .map_err(|_| invalid("catalog_refresh_job_source.duration", value.to_string()))
            })
            .transpose()?,
        error_code: row.try_get("error_code")?,
        error_message: row.try_get("error_message")?,
    })
}

fn timestamp(row: &SqliteRow, field: &'static str) -> Result<OffsetDateTime, StorageError> {
    let value: String = row.try_get(field)?;
    OffsetDateTime::parse(&value, &Rfc3339)
        .map_err(|source| StorageError::ParseTimestamp { field, source })
}

fn optional_timestamp(
    row: &SqliteRow,
    field: &'static str,
) -> Result<Option<OffsetDateTime>, StorageError> {
    let value: Option<String> = row.try_get(field)?;
    value
        .map(|value| {
            OffsetDateTime::parse(&value, &Rfc3339)
                .map_err(|source| StorageError::ParseTimestamp { field, source })
        })
        .transpose()
}

pub(super) fn validate_sources(sources: &[String]) -> Result<(), StorageError> {
    if sources.is_empty() {
        return Err(StorageError::EmptyField { field: "sources" });
    }
    let mut seen = HashSet::new();
    for source in sources {
        if source.trim().is_empty() || !seen.insert(source) {
            return Err(invalid("catalog_refresh_job.source", source));
        }
    }
    Ok(())
}

pub(super) fn stored_usize(value: usize, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

pub(super) fn stored_u64(value: u64, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

pub(super) fn invalid(field: &'static str, value: impl Into<String>) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.into(),
    }
}
