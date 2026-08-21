mod read;
mod write;

use sqlx::{Row, sqlite::SqliteRow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{RankingNamespace, RankingSnapshot};
use crate::StorageError;

pub(super) fn snapshot_from_row(row: &SqliteRow) -> Result<RankingSnapshot, StorageError> {
    let namespace_text: String = row.try_get("namespace")?;
    let namespace = RankingNamespace::from_stored(&namespace_text)
        .ok_or_else(|| invalid("ranking_snapshots.namespace", namespace_text))?;
    let group_id_text: String = row.try_get("group_id")?;
    Ok(RankingSnapshot {
        namespace,
        group_id: maimai_core::GroupId::new(&group_id_text)
            .map_err(|_| invalid("ranking_snapshots.group_id", group_id_text))?,
        generation: unsigned(row, "generation")?,
        fetched_at: timestamp(row, "fetched_at")?,
        next_reset_at: timestamp(row, "next_reset_at")?,
        member_count: unsigned_u32(row, "member_count")?,
        success_count: unsigned_u32(row, "success_count")?,
        failure_count: unsigned_u32(row, "failure_count")?,
        skipped_count: unsigned_u32(row, "skipped_count")?,
        cache_hit_count: unsigned_u32(row, "cache_hit_count")?,
        shared_fetch_count: unsigned_u32(row, "shared_fetch_count")?,
    })
}

pub(super) fn timestamp(
    row: &SqliteRow,
    field: &'static str,
) -> Result<OffsetDateTime, StorageError> {
    let value: String = row.try_get(field)?;
    OffsetDateTime::parse(&value, &Rfc3339)
        .map_err(|source| StorageError::ParseTimestamp { field, source })
}

pub(super) fn unsigned(row: &SqliteRow, field: &'static str) -> Result<u64, StorageError> {
    let value: i64 = row.try_get(field)?;
    u64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

pub(super) fn unsigned_u32(row: &SqliteRow, field: &'static str) -> Result<u32, StorageError> {
    let value: i64 = row.try_get(field)?;
    u32::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

pub(super) fn stored_u64(value: u64, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

pub(super) const fn stored_u32(value: u32) -> i64 {
    value as i64
}

pub(super) fn invalid(field: &'static str, value: impl Into<String>) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.into(),
    }
}
