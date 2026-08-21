use sqlx::{Row, sqlite::SqliteRow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    IdentityJob, IdentityJobError, IdentityJobErrorCode, IdentityJobProgress, IdentityJobStart,
    IdentityJobStatus, IdentityRefreshReason, IdentityStats, metadata::stored_integer,
};
use crate::{StateStore, StorageError};

impl StateStore {
    pub async fn start_identity_job(
        &self,
        reason: IdentityRefreshReason,
        started_at: OffsetDateTime,
    ) -> Result<IdentityJobStart, StorageError> {
        let timestamp = started_at.format(&Rfc3339)?;
        let row = sqlx::query(
            r#"
            INSERT INTO identity_refresh_job (
                singleton, generation, status, started_at, refresh_reason, message
            ) VALUES (1, 1, 'running', ?, ?, 'QQ 身份缓存刷新已启动。')
            ON CONFLICT (singleton) DO UPDATE SET
                generation = identity_refresh_job.generation + 1,
                status = excluded.status,
                started_at = excluded.started_at,
                finished_at = NULL,
                refresh_reason = excluded.refresh_reason,
                message = excluded.message,
                processed_groups = 0,
                total_groups = NULL,
                friend_count = NULL,
                current_group_id = NULL,
                current_group_name = NULL,
                unique_users = NULL,
                stats_friend_count = NULL,
                stats_group_count = NULL,
                stats_group_member_rows = NULL,
                stats_unique_users = NULL,
                error_code = NULL,
                error_message = NULL,
                error_status = NULL,
                error_body = NULL
            WHERE identity_refresh_job.status != 'running'
            RETURNING *
            "#,
        )
        .bind(timestamp)
        .bind(reason.as_str())
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            return Ok(IdentityJobStart::Started(job_from_row(row)?));
        }
        let running = self
            .identity_job()
            .await?
            .ok_or_else(|| invalid("identity_job", "missing after CAS"))?;
        Ok(IdentityJobStart::AlreadyRunning(running))
    }

    pub async fn update_identity_job_progress(
        &self,
        generation: u64,
        message: &str,
        progress: &IdentityJobProgress,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE identity_refresh_job SET
                message = ?, processed_groups = ?, total_groups = ?, friend_count = ?,
                current_group_id = ?, current_group_name = ?, unique_users = ?
            WHERE singleton = 1 AND generation = ? AND status = 'running'
            "#,
        )
        .bind(message)
        .bind(stored_integer(
            progress.processed_groups,
            "processed_groups",
        )?)
        .bind(optional_integer(progress.total_groups, "total_groups")?)
        .bind(optional_integer(progress.friend_count, "friend_count")?)
        .bind(&progress.current_group_id)
        .bind(&progress.current_group_name)
        .bind(optional_integer(progress.unique_users, "unique_users")?)
        .bind(stored_integer(generation, "identity_job.generation")?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn complete_identity_job(
        &self,
        generation: u64,
        finished_at: OffsetDateTime,
        stats: IdentityStats,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE identity_refresh_job SET
                status = 'completed', finished_at = ?, message = 'QQ 身份缓存刷新完成。',
                stats_friend_count = ?, stats_group_count = ?,
                stats_group_member_rows = ?, stats_unique_users = ?,
                error_code = NULL, error_message = NULL, error_status = NULL, error_body = NULL
            WHERE singleton = 1 AND generation = ? AND status = 'running'
            "#,
        )
        .bind(finished_at.format(&Rfc3339)?)
        .bind(stored_integer(stats.friend_count, "stats.friend_count")?)
        .bind(stored_integer(stats.group_count, "stats.group_count")?)
        .bind(stored_integer(
            stats.group_member_rows,
            "stats.group_member_rows",
        )?)
        .bind(stored_integer(stats.unique_users, "stats.unique_users")?)
        .bind(stored_integer(generation, "identity_job.generation")?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn fail_identity_job(
        &self,
        generation: u64,
        finished_at: OffsetDateTime,
        error: &IdentityJobError,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE identity_refresh_job SET
                status = 'failed', finished_at = ?, message = ?,
                error_code = ?, error_message = ?, error_status = ?, error_body = ?
            WHERE singleton = 1 AND generation = ? AND status = 'running'
            "#,
        )
        .bind(finished_at.format(&Rfc3339)?)
        .bind(&error.message)
        .bind(error.code.as_str())
        .bind(&error.message)
        .bind(error.status.map(i64::from))
        .bind(&error.body)
        .bind(stored_integer(generation, "identity_job.generation")?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn interrupt_running_identity_job(
        &self,
        finished_at: OffsetDateTime,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE identity_refresh_job SET
                status = 'interrupted', finished_at = ?,
                message = '上次 QQ 身份缓存刷新因进程重启中断。'
            WHERE singleton = 1 AND status = 'running'
            "#,
        )
        .bind(finished_at.format(&Rfc3339)?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn identity_job(&self) -> Result<Option<IdentityJob>, StorageError> {
        sqlx::query("SELECT * FROM identity_refresh_job WHERE singleton = 1")
            .fetch_optional(&self.pool)
            .await?
            .map(job_from_row)
            .transpose()
    }
}

fn job_from_row(row: SqliteRow) -> Result<IdentityJob, StorageError> {
    let status_text: String = row.try_get("status")?;
    let reason_text: String = row.try_get("refresh_reason")?;
    let error_code: Option<String> = row.try_get("error_code")?;
    Ok(IdentityJob {
        generation: unsigned(&row, "generation")?,
        status: IdentityJobStatus::from_stored(&status_text)?,
        started_at: timestamp(&row, "started_at")?,
        finished_at: optional_timestamp(&row, "finished_at")?,
        refresh_reason: IdentityRefreshReason::from_stored(&reason_text)?,
        message: row.try_get("message")?,
        progress: IdentityJobProgress {
            processed_groups: unsigned(&row, "processed_groups")?,
            total_groups: optional_unsigned(&row, "total_groups")?,
            friend_count: optional_unsigned(&row, "friend_count")?,
            current_group_id: row.try_get("current_group_id")?,
            current_group_name: row.try_get("current_group_name")?,
            unique_users: optional_unsigned(&row, "unique_users")?,
        },
        stats: optional_stats(&row)?,
        error: error_code
            .map(|code| -> Result<IdentityJobError, StorageError> {
                Ok(IdentityJobError {
                    code: IdentityJobErrorCode::from_stored(&code)?,
                    message: row.try_get("error_message")?,
                    status: optional_u16(&row, "error_status")?,
                    body: row.try_get("error_body")?,
                })
            })
            .transpose()?,
    })
}

fn optional_stats(row: &SqliteRow) -> Result<Option<IdentityStats>, StorageError> {
    let friend_count = optional_unsigned(row, "stats_friend_count")?;
    let Some(friend_count) = friend_count else {
        return Ok(None);
    };
    Ok(Some(IdentityStats {
        friend_count,
        group_count: optional_unsigned(row, "stats_group_count")?
            .ok_or_else(|| invalid("stats_group_count", "NULL"))?,
        group_member_rows: optional_unsigned(row, "stats_group_member_rows")?
            .ok_or_else(|| invalid("stats_group_member_rows", "NULL"))?,
        unique_users: optional_unsigned(row, "stats_unique_users")?
            .ok_or_else(|| invalid("stats_unique_users", "NULL"))?,
    }))
}

fn optional_integer(value: Option<u64>, field: &'static str) -> Result<Option<i64>, StorageError> {
    value.map(|value| stored_integer(value, field)).transpose()
}

fn unsigned(row: &SqliteRow, field: &'static str) -> Result<u64, StorageError> {
    let value: i64 = row.try_get(field)?;
    u64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

fn optional_unsigned(row: &SqliteRow, field: &'static str) -> Result<Option<u64>, StorageError> {
    let value: Option<i64> = row.try_get(field)?;
    value
        .map(|value| u64::try_from(value).map_err(|_| invalid(field, value.to_string())))
        .transpose()
}

fn optional_u16(row: &SqliteRow, field: &'static str) -> Result<Option<u16>, StorageError> {
    let value: Option<i64> = row.try_get(field)?;
    value
        .map(|value| u16::try_from(value).map_err(|_| invalid(field, value.to_string())))
        .transpose()
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

fn invalid(field: &'static str, value: impl Into<String>) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.into(),
    }
}
