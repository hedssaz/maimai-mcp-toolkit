use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    RankingJob, RankingJobError, RankingJobErrorCode, RankingJobProgress, RankingJobStart,
    RankingJobStatus, RankingNamespace, RankingRefreshReason,
    snapshot::{invalid, stored_u32, stored_u64},
};
use crate::{StateStore, StorageError};

impl StateStore {
    pub async fn start_ranking_job(
        &self,
        namespace: RankingNamespace,
        group_id: &maimai_core::GroupId,
        reason: RankingRefreshReason,
        started_at: OffsetDateTime,
    ) -> Result<RankingJobStart, StorageError> {
        let row = sqlx::query(
            r#"INSERT INTO ranking_jobs (
                namespace, group_id, generation, status, started_at, refresh_reason, message
            ) VALUES (?, ?, 1, 'running', ?, ?, ?)
            ON CONFLICT (namespace, group_id) DO UPDATE SET
                generation = ranking_jobs.generation + 1,
                status = 'running', started_at = excluded.started_at, finished_at = NULL,
                refresh_reason = excluded.refresh_reason, message = excluded.message,
                processed_count = 0, total_count = NULL, cached_count = 0,
                progress_skipped_count = 0, transient_failure_count = 0, current_qq = NULL,
                member_count = NULL, success_count = NULL, skipped_count = NULL,
                error_code = NULL, error_message = NULL, error_status = NULL, error_body = NULL
            WHERE ranking_jobs.status != 'running'
            RETURNING *"#,
        )
        .bind(namespace.as_str())
        .bind(group_id.as_str())
        .bind(started_at.format(&Rfc3339)?)
        .bind(reason.as_str())
        .bind(start_message(namespace))
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            return Ok(RankingJobStart::Started(job_from_row(row)?));
        }
        let running = self
            .ranking_job(namespace, group_id)
            .await?
            .ok_or_else(|| invalid("ranking_jobs", "missing after CAS"))?;
        Ok(RankingJobStart::AlreadyRunning(running))
    }

    pub async fn update_ranking_job_progress(
        &self,
        namespace: RankingNamespace,
        group_id: &maimai_core::GroupId,
        generation: u64,
        message: &str,
        progress: &RankingJobProgress,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"UPDATE ranking_jobs SET
                message = ?, processed_count = ?, total_count = ?, cached_count = ?,
                progress_skipped_count = ?, transient_failure_count = ?, current_qq = ?
            WHERE namespace = ? AND group_id = ? AND generation = ? AND status = 'running'"#,
        )
        .bind(message)
        .bind(stored_u32(progress.processed_count))
        .bind(progress.total_count.map(stored_u32))
        .bind(stored_u32(progress.cached_count))
        .bind(stored_u32(progress.skipped_count))
        .bind(stored_u32(progress.transient_failure_count))
        .bind(progress.current_qq.as_ref().map(maimai_core::QqId::as_str))
        .bind(namespace.as_str())
        .bind(group_id.as_str())
        .bind(stored_u64(generation, "ranking_jobs.generation")?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn fail_ranking_job(
        &self,
        namespace: RankingNamespace,
        group_id: &maimai_core::GroupId,
        generation: u64,
        finished_at: OffsetDateTime,
        error: &RankingJobError,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"UPDATE ranking_jobs SET status = 'failed', finished_at = ?, message = ?,
                error_code = ?, error_message = ?, error_status = ?, error_body = ?
            WHERE namespace = ? AND group_id = ? AND generation = ? AND status = 'running'"#,
        )
        .bind(finished_at.format(&Rfc3339)?)
        .bind(&error.message)
        .bind(error.code.as_str())
        .bind(&error.message)
        .bind(error.status.map(i64::from))
        .bind(&error.body)
        .bind(namespace.as_str())
        .bind(group_id.as_str())
        .bind(stored_u64(generation, "ranking_jobs.generation")?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn interrupt_running_ranking_jobs(
        &self,
        finished_at: OffsetDateTime,
    ) -> Result<u64, StorageError> {
        let result = sqlx::query(
            r#"UPDATE ranking_jobs SET status = 'interrupted', finished_at = ?,
                message = '上次群榜刷新因进程重启中断。'
            WHERE status = 'running'"#,
        )
        .bind(finished_at.format(&Rfc3339)?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn ranking_job(
        &self,
        namespace: RankingNamespace,
        group_id: &maimai_core::GroupId,
    ) -> Result<Option<RankingJob>, StorageError> {
        sqlx::query("SELECT * FROM ranking_jobs WHERE namespace = ? AND group_id = ?")
            .bind(namespace.as_str())
            .bind(group_id.as_str())
            .fetch_optional(&self.pool)
            .await?
            .map(job_from_row)
            .transpose()
    }
}

pub(super) async fn complete_job_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &super::RankingSnapshot,
    finished_at: OffsetDateTime,
) -> Result<(), StorageError> {
    let result = sqlx::query(
        r#"UPDATE ranking_jobs SET status = 'completed', finished_at = ?,
            message = ?, member_count = ?, success_count = ?, skipped_count = ?,
            processed_count = ?, total_count = ?, cached_count = ?,
            error_code = NULL, error_message = NULL, error_status = NULL, error_body = NULL
        WHERE namespace = ? AND group_id = ? AND generation = ? AND status = 'running'"#,
    )
    .bind(finished_at.format(&Rfc3339)?)
    .bind(complete_message(snapshot.namespace))
    .bind(stored_u32(snapshot.member_count))
    .bind(stored_u32(snapshot.success_count))
    .bind(stored_u32(snapshot.skipped_count))
    .bind(stored_u32(snapshot.member_count))
    .bind(stored_u32(snapshot.member_count))
    .bind(stored_u32(snapshot.success_count))
    .bind(snapshot.namespace.as_str())
    .bind(snapshot.group_id.as_str())
    .bind(stored_u64(snapshot.generation, "ranking_jobs.generation")?)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() == 1 {
        Ok(())
    } else {
        Err(invalid("ranking_jobs", "terminal generation guard failed"))
    }
}

fn job_from_row(row: SqliteRow) -> Result<RankingJob, StorageError> {
    let namespace_text: String = row.try_get("namespace")?;
    let status_text: String = row.try_get("status")?;
    let reason_text: String = row.try_get("refresh_reason")?;
    let group_id_text: String = row.try_get("group_id")?;
    let error_code: Option<String> = row.try_get("error_code")?;
    Ok(RankingJob {
        namespace: RankingNamespace::from_stored(&namespace_text)
            .ok_or_else(|| invalid("ranking_jobs.namespace", namespace_text))?,
        group_id: maimai_core::GroupId::new(&group_id_text)
            .map_err(|_| invalid("ranking_jobs.group_id", group_id_text))?,
        generation: required_u64(&row, "generation")?,
        status: RankingJobStatus::from_stored(&status_text)
            .ok_or_else(|| invalid("ranking_jobs.status", status_text))?,
        started_at: required_timestamp(&row, "started_at")?,
        finished_at: optional_timestamp(&row, "finished_at")?,
        refresh_reason: RankingRefreshReason::from_stored(&reason_text)
            .ok_or_else(|| invalid("ranking_jobs.refresh_reason", reason_text))?,
        message: row.try_get("message")?,
        progress: RankingJobProgress {
            processed_count: required_u32(&row, "processed_count")?,
            total_count: optional_u32(&row, "total_count")?,
            cached_count: required_u32(&row, "cached_count")?,
            skipped_count: required_u32(&row, "progress_skipped_count")?,
            transient_failure_count: required_u32(&row, "transient_failure_count")?,
            current_qq: row
                .try_get::<Option<String>, _>("current_qq")?
                .map(maimai_core::QqId::new)
                .transpose()
                .map_err(|_| invalid("ranking_jobs.current_qq", "invalid QQ"))?,
        },
        member_count: optional_u32(&row, "member_count")?,
        success_count: optional_u32(&row, "success_count")?,
        skipped_count: optional_u32(&row, "skipped_count")?,
        error: error_code
            .map(|code| -> Result<RankingJobError, StorageError> {
                Ok(RankingJobError {
                    code: RankingJobErrorCode::from_stored(&code)
                        .ok_or_else(|| invalid("ranking_jobs.error_code", code))?,
                    message: row.try_get("error_message")?,
                    status: optional_u16(&row, "error_status")?,
                    body: row.try_get("error_body")?,
                })
            })
            .transpose()?,
    })
}

fn start_message(namespace: RankingNamespace) -> &'static str {
    match namespace {
        RankingNamespace::B50 => "后台刷新已启动，正在拉取群成员并查询 B50。",
        RankingNamespace::SongScore => "单曲成绩榜后台刷新已启动，正在拉取全群完整成绩。",
    }
}

fn complete_message(namespace: RankingNamespace) -> &'static str {
    match namespace {
        RankingNamespace::B50 => "后台刷新已完成，可读取缓存榜单。",
        RankingNamespace::SongScore => "单曲成绩榜后台刷新已完成。",
    }
}

fn required_timestamp(
    row: &SqliteRow,
    field: &'static str,
) -> Result<OffsetDateTime, StorageError> {
    let value: String = row.try_get(field)?;
    OffsetDateTime::parse(&value, &Rfc3339)
        .map_err(|source| StorageError::ParseTimestamp { field, source })
}

fn optional_timestamp(
    row: &SqliteRow,
    field: &'static str,
) -> Result<Option<OffsetDateTime>, StorageError> {
    row.try_get::<Option<String>, _>(field)?
        .map(|value| {
            OffsetDateTime::parse(&value, &Rfc3339)
                .map_err(|source| StorageError::ParseTimestamp { field, source })
        })
        .transpose()
}

fn required_u64(row: &SqliteRow, field: &'static str) -> Result<u64, StorageError> {
    let value: i64 = row.try_get(field)?;
    u64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

fn required_u32(row: &SqliteRow, field: &'static str) -> Result<u32, StorageError> {
    let value: i64 = row.try_get(field)?;
    u32::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

fn optional_u32(row: &SqliteRow, field: &'static str) -> Result<Option<u32>, StorageError> {
    row.try_get::<Option<i64>, _>(field)?
        .map(|value| u32::try_from(value).map_err(|_| invalid(field, value.to_string())))
        .transpose()
}

fn optional_u16(row: &SqliteRow, field: &'static str) -> Result<Option<u16>, StorageError> {
    row.try_get::<Option<i64>, _>(field)?
        .map(|value| u16::try_from(value).map_err(|_| invalid(field, value.to_string())))
        .transpose()
}
