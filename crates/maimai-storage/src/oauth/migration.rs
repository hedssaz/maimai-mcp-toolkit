use maimai_core::QqId;
use serde_json::{Map, Value};
use sqlx::{Row, Sqlite, Transaction};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::StorageError;

const MIGRATION_NAME: &str = "lxns_oauth_legacy_binding_v1";
const LEGACY_DEFAULT_CLIENT_ID: &str = "f046fb83-f5c5-436d-bdfb-45c555638aa4";
const LEGACY_DEFAULT_SCOPE: &str = "write_player read_user_profile read_player";
const MAX_TOKEN_BYTES: usize = 32 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024;

pub(super) async fn migrate_legacy_oauth_tokens(
    pool: &sqlx::SqlitePool,
) -> Result<(), StorageError> {
    let now = OffsetDateTime::now_utc();
    let now_text = now.format(&Rfc3339)?;
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    if migration_applied(&mut transaction).await? {
        transaction.commit().await?;
        return Ok(());
    }

    let mut imported = 0_u64;
    let mut skipped = 0_u64;
    if table_exists(&mut transaction, "user_bindings").await? {
        let rows = sqlx::query("SELECT qq, record_json, updated_at FROM user_bindings ORDER BY qq")
            .fetch_all(&mut *transaction)
            .await?;
        for row in rows {
            let qq = row.try_get::<String, _>("qq")?;
            let record = row.try_get::<String, _>("record_json")?;
            let updated_at = row.try_get::<String, _>("updated_at").ok();
            let Some(token) =
                decode_token(&qq, &record, updated_at.as_deref(), now.unix_timestamp())
            else {
                if contains_oauth_record(&record) {
                    skipped = skipped.saturating_add(1);
                }
                continue;
            };
            let result = sqlx::query(
                r#"
                INSERT INTO lxns_oauth_tokens (
                    subject, generation, access_token, refresh_token, token_type,
                    scope, client_id, expires_at, bound_at, updated_at
                ) VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT (subject) DO NOTHING
                "#,
            )
            .bind(token.subject)
            .bind(token.access_token)
            .bind(token.refresh_token)
            .bind(token.token_type)
            .bind(token.scope)
            .bind(token.client_id)
            .bind(token.expires_at)
            .bind(token.bound_at)
            .bind(token.updated_at)
            .execute(&mut *transaction)
            .await?;
            imported = imported.saturating_add(result.rows_affected());
        }
    }

    sqlx::query(
        r#"
        INSERT INTO maimai_storage_migrations (
            name, applied_at, imported_count, skipped_count
        ) VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(MIGRATION_NAME)
    .bind(now_text)
    .bind(stored_count(imported, "oauth_legacy.imported_count")?)
    .bind(stored_count(skipped, "oauth_legacy.skipped_count")?)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

struct LegacyToken {
    subject: String,
    access_token: String,
    refresh_token: String,
    token_type: String,
    scope: Option<String>,
    client_id: String,
    expires_at: Option<i64>,
    bound_at: i64,
    updated_at: i64,
}

fn decode_token(
    qq: &str,
    record: &str,
    row_updated_at: Option<&str>,
    now: i64,
) -> Option<LegacyToken> {
    if qq.len() > 256 {
        return None;
    }
    let subject = QqId::new(qq.to_owned()).ok()?.as_str().to_owned();
    let record = serde_json::from_str::<Value>(record).ok()?;
    let oauth = record.get("lxnsOAuth")?.as_object()?;
    let access_token = secret(oauth, "accessToken", false)?;
    let refresh_token = secret(oauth, "refreshToken", true)?;
    let token_type = text(oauth, "tokenType", MAX_TEXT_BYTES).unwrap_or_else(|| "Bearer".into());
    let client_id = text(oauth, "clientId", MAX_TEXT_BYTES)
        .unwrap_or_else(|| LEGACY_DEFAULT_CLIENT_ID.to_owned());
    let scope = Some(
        text(oauth, "scope", MAX_TEXT_BYTES).unwrap_or_else(|| LEGACY_DEFAULT_SCOPE.to_owned()),
    );
    let fallback = timestamp(row_updated_at).unwrap_or(now);
    Some(LegacyToken {
        subject,
        access_token,
        refresh_token,
        token_type,
        scope,
        client_id,
        expires_at: oauth.get("expiresAt").and_then(timestamp_value),
        bound_at: oauth
            .get("boundAt")
            .and_then(timestamp_value)
            .unwrap_or(fallback),
        updated_at: oauth
            .get("updatedAt")
            .and_then(timestamp_value)
            .unwrap_or(fallback),
    })
}

fn contains_oauth_record(record: &str) -> bool {
    serde_json::from_str::<Value>(record)
        .ok()
        .and_then(|value| value.get("lxnsOAuth").cloned())
        .is_some()
}

fn secret(record: &Map<String, Value>, field: &str, allow_empty: bool) -> Option<String> {
    let value = match record.get(field) {
        Some(value) => value.as_str()?.trim().to_owned(),
        None if allow_empty => String::new(),
        None => return None,
    };
    let valid_length = value.len() <= MAX_TOKEN_BYTES;
    let valid_chars = !value.chars().any(char::is_control);
    (valid_length && valid_chars && (allow_empty || !value.is_empty())).then_some(value)
}

fn text(record: &Map<String, Value>, field: &str, max_bytes: usize) -> Option<String> {
    let value = record.get(field)?.as_str()?.trim().to_owned();
    (!value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control))
        .then_some(value)
}

fn timestamp_value(value: &Value) -> Option<i64> {
    match value {
        Value::String(value) => timestamp(Some(value)),
        Value::Number(value) => value.as_i64(),
        _ => None,
    }
}

fn timestamp(value: Option<&str>) -> Option<i64> {
    OffsetDateTime::parse(value?, &Rfc3339)
        .ok()
        .map(|value| value.unix_timestamp())
}

async fn migration_applied(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<bool, StorageError> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT 1 FROM maimai_storage_migrations WHERE name = ?")
            .bind(MIGRATION_NAME)
            .fetch_optional(&mut **transaction)
            .await?
            .is_some(),
    )
}

async fn table_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    name: &str,
) -> Result<bool, StorageError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?",
    )
    .bind(name)
    .fetch_optional(&mut **transaction)
    .await?
    .is_some())
}

fn stored_count(value: u64, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::InvalidStoredValue {
        field,
        value: value.to_string(),
    })
}
