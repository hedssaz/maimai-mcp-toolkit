use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{IdentityMetadata, IdentityStats};
use crate::StorageError;

pub(super) async fn next_generation(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<u64, StorageError> {
    let current = sqlx::query_scalar::<_, i64>(
        "SELECT generation FROM identity_metadata WHERE singleton = 1",
    )
    .fetch_optional(&mut **transaction)
    .await?
    .unwrap_or(0);
    let current = u64::try_from(current).map_err(|_| invalid("generation", current.to_string()))?;
    current
        .checked_add(1)
        .ok_or_else(|| invalid("generation", current.to_string()))
}

pub(super) async fn identity_stats(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<IdentityStats, StorageError> {
    Ok(IdentityStats {
        friend_count: count(
            transaction,
            "SELECT COUNT(*) FROM identity_users WHERE is_friend = 1",
        )
        .await?,
        group_count: count(transaction, "SELECT COUNT(*) FROM identity_groups").await?,
        group_member_rows: count(transaction, "SELECT COUNT(*) FROM identity_members").await?,
        unique_users: count(transaction, "SELECT COUNT(*) FROM identity_users").await?,
    })
}

pub(super) fn metadata_from_row(row: SqliteRow) -> Result<IdentityMetadata, StorageError> {
    Ok(IdentityMetadata {
        fetched_at: optional_timestamp(&row, "fetched_at")?,
        updated_at: timestamp(&row, "updated_at")?,
        generation: unsigned(&row, "generation")?,
        stats: IdentityStats {
            friend_count: unsigned(&row, "friend_count")?,
            group_count: unsigned(&row, "group_count")?,
            group_member_rows: unsigned(&row, "group_member_rows")?,
            unique_users: unsigned(&row, "unique_users")?,
        },
    })
}

pub(super) fn stored_integer(value: u64, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

async fn count(
    transaction: &mut Transaction<'_, Sqlite>,
    query: &'static str,
) -> Result<u64, StorageError> {
    let value = sqlx::query_scalar::<_, i64>(query)
        .fetch_one(&mut **transaction)
        .await?;
    u64::try_from(value).map_err(|_| invalid("count", value.to_string()))
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

fn unsigned(row: &SqliteRow, field: &'static str) -> Result<u64, StorageError> {
    let value: i64 = row.try_get(field)?;
    u64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

fn invalid(field: &'static str, value: impl Into<String>) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.into(),
    }
}
