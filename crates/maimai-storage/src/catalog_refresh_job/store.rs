use std::collections::HashSet;

use sqlx::{Row, Sqlite, Transaction};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::decode::{
    invalid, job_from_row, source_from_row, stored_u64, stored_usize, validate_sources,
};
use super::{
    CatalogRefreshJob, CatalogRefreshJobId, CatalogRefreshJobStart, CatalogRefreshJobStatus,
    CatalogRefreshJobStore, CatalogRefreshSourceUpdate, CatalogRefreshTerminalUpdate,
};
use crate::StorageError;

impl CatalogRefreshJobStore {
    pub async fn start_catalog_refresh_job(
        &self,
        sources: &[String],
        due_sources: &[String],
        created_at: OffsetDateTime,
    ) -> Result<CatalogRefreshJobStart, StorageError> {
        validate_sources(sources)?;
        let due = due_sources.iter().collect::<HashSet<_>>();
        if due.len() != due_sources.len() || due.iter().any(|source| !sources.contains(source)) {
            return Err(invalid("catalog_refresh_job.due_sources", "invalid subset"));
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(id) = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM catalog_refresh_jobs WHERE status IN ('queued', 'running') LIMIT 1",
        )
        .fetch_optional(&mut *transaction)
        .await?
        {
            transaction.commit().await?;
            let id = CatalogRefreshJobId::from_stored(id)?;
            let job = self
                .catalog_refresh_job(id)
                .await?
                .ok_or_else(|| invalid("catalog_refresh_job", "active row missing"))?;
            return Ok(CatalogRefreshJobStart::AlreadyRunning(job));
        }
        let id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO catalog_refresh_jobs (status, created_at, message)
            VALUES ('queued', ?, '后台刷新已排队。')
            RETURNING id
            "#,
        )
        .bind(created_at.format(&Rfc3339)?)
        .fetch_one(&mut *transaction)
        .await?;
        for (position, source) in sources.iter().enumerate() {
            sqlx::query(
                r#"
                INSERT INTO catalog_refresh_job_sources (
                    job_id, position, source, due, status
                ) VALUES (?, ?, ?, ?, ?)
                "#,
            )
            .bind(id)
            .bind(stored_usize(
                position,
                "catalog_refresh_job_source.position",
            )?)
            .bind(source)
            .bind(i64::from(due.contains(source)))
            .bind(if due.contains(source) {
                "queued"
            } else {
                "skipped"
            })
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        let id = CatalogRefreshJobId::from_stored(id)?;
        let job = self
            .catalog_refresh_job(id)
            .await?
            .ok_or_else(|| invalid("catalog_refresh_job", "inserted row missing"))?;
        Ok(CatalogRefreshJobStart::Started(job))
    }

    pub async fn mark_catalog_refresh_job_running(
        &self,
        id: CatalogRefreshJobId,
        started_at: OffsetDateTime,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE catalog_refresh_jobs
            SET status = 'running', started_at = ?, message = '并发刷新中...'
            WHERE id = ? AND status = 'queued'
            "#,
        )
        .bind(started_at.format(&Rfc3339)?)
        .bind(id.value())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn plan_catalog_refresh_job(
        &self,
        id: CatalogRefreshJobId,
        due: &[String],
        skipped: &[String],
    ) -> Result<bool, StorageError> {
        let mut transaction = self.pool.begin().await?;
        if !job_is_running(&mut transaction, id).await? {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            "UPDATE catalog_refresh_job_sources SET due = 0, status = 'queued' WHERE job_id = ?",
        )
        .bind(id.value())
        .execute(&mut *transaction)
        .await?;
        for source in due {
            update_planned_source(&mut transaction, id, source, true, "pending").await?;
        }
        for source in skipped {
            update_planned_source(&mut transaction, id, source, false, "skipped").await?;
        }
        sqlx::query(
            "UPDATE catalog_refresh_jobs SET message = ? WHERE id = ? AND status = 'running'",
        )
        .bind(format!(
            "刷新进度: {}/{} (成功 0, 失败 0)",
            skipped.len(),
            due.len() + skipped.len()
        ))
        .bind(id.value())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_catalog_refresh_source(
        &self,
        id: CatalogRefreshJobId,
        update: &CatalogRefreshSourceUpdate,
    ) -> Result<bool, StorageError> {
        if !update.status.completed() {
            return Err(invalid(
                "catalog_refresh_source.status",
                update.status.as_str(),
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE catalog_refresh_job_sources SET
                status = ?, duration_millis = ?, error_code = ?, error_message = ?
            WHERE job_id = ? AND source = ?
              AND EXISTS (
                  SELECT 1 FROM catalog_refresh_jobs
                  WHERE id = ? AND status = 'running'
              )
            "#,
        )
        .bind(update.status.as_str())
        .bind(stored_u64(update.duration_millis, "duration_millis")?)
        .bind(&update.error_code)
        .bind(&update.error_message)
        .bind(id.value())
        .bind(&update.source)
        .bind(id.value())
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }
        let (completed, succeeded, failed, total) = progress_counts(&mut transaction, id).await?;
        sqlx::query(
            "UPDATE catalog_refresh_jobs SET message = ? WHERE id = ? AND status = 'running'",
        )
        .bind(format!(
            "刷新进度: {completed}/{total} (成功 {succeeded}, 失败 {failed})"
        ))
        .bind(id.value())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn finish_catalog_refresh_job(
        &self,
        id: CatalogRefreshJobId,
        finished_at: OffsetDateTime,
        update: &CatalogRefreshTerminalUpdate,
    ) -> Result<bool, StorageError> {
        if !update.status.is_terminal() || update.status == CatalogRefreshJobStatus::Interrupted {
            return Err(invalid(
                "catalog_refresh_job.status",
                update.status.as_str(),
            ));
        }
        let result = sqlx::query(
            r#"
            UPDATE catalog_refresh_jobs SET
                status = ?, outcome = ?, finished_at = ?, message = ?,
                error_code = ?, error_message = ?
            WHERE id = ? AND status = 'running'
            "#,
        )
        .bind(update.status.as_str())
        .bind(update.outcome.as_str())
        .bind(finished_at.format(&Rfc3339)?)
        .bind(&update.message)
        .bind(&update.error_code)
        .bind(&update.error_message)
        .bind(id.value())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn fail_active_catalog_refresh_job(
        &self,
        id: CatalogRefreshJobId,
        finished_at: OffsetDateTime,
        error_code: &str,
        error_message: &str,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE catalog_refresh_jobs SET
                status = 'failed', outcome = 'failed', finished_at = ?,
                message = '刷新任务异常终止。', error_code = ?, error_message = ?
            WHERE id = ? AND status IN ('queued', 'running')
            "#,
        )
        .bind(finished_at.format(&Rfc3339)?)
        .bind(error_code)
        .bind(error_message)
        .bind(id.value())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn catalog_refresh_job(
        &self,
        id: CatalogRefreshJobId,
    ) -> Result<Option<CatalogRefreshJob>, StorageError> {
        let Some(row) = sqlx::query("SELECT * FROM catalog_refresh_jobs WHERE id = ?")
            .bind(id.value())
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        let sources = sqlx::query(
            "SELECT * FROM catalog_refresh_job_sources WHERE job_id = ? ORDER BY position",
        )
        .bind(id.value())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(source_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(job_from_row(row, sources)?))
    }

    pub async fn active_catalog_refresh_job(
        &self,
    ) -> Result<Option<CatalogRefreshJob>, StorageError> {
        let Some(id) = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM catalog_refresh_jobs WHERE status IN ('queued', 'running') LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        self.catalog_refresh_job(CatalogRefreshJobId::from_stored(id)?)
            .await
    }
}

async fn update_planned_source(
    transaction: &mut Transaction<'_, Sqlite>,
    id: CatalogRefreshJobId,
    source: &str,
    due: bool,
    status: &str,
) -> Result<(), StorageError> {
    let result = sqlx::query(
        "UPDATE catalog_refresh_job_sources SET due = ?, status = ? WHERE job_id = ? AND source = ?",
    )
    .bind(i64::from(due))
    .bind(status)
    .bind(id.value())
    .bind(source)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(invalid("catalog_refresh_job.source", source));
    }
    Ok(())
}

async fn job_is_running(
    transaction: &mut Transaction<'_, Sqlite>,
    id: CatalogRefreshJobId,
) -> Result<bool, StorageError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM catalog_refresh_jobs WHERE id = ? AND status = 'running'",
    )
    .bind(id.value())
    .fetch_one(&mut **transaction)
    .await?
        == 1)
}

async fn progress_counts(
    transaction: &mut Transaction<'_, Sqlite>,
    id: CatalogRefreshJobId,
) -> Result<(i64, i64, i64, i64), StorageError> {
    let row = sqlx::query(
        r#"
        SELECT
            SUM(CASE WHEN status IN (
                'updated', 'not_modified', 'skipped', 'failed',
                'disk_updated_pending_reload'
            ) THEN 1 ELSE 0 END) AS completed,
            SUM(CASE WHEN status IN ('updated', 'not_modified', 'skipped') THEN 1 ELSE 0 END) AS succeeded,
            SUM(CASE WHEN status IN ('failed', 'disk_updated_pending_reload') THEN 1 ELSE 0 END) AS failed,
            COUNT(*) AS total
        FROM catalog_refresh_job_sources WHERE job_id = ?
        "#,
    )
    .bind(id.value())
    .fetch_one(&mut **transaction)
    .await?;
    Ok((
        row.try_get("completed")?,
        row.try_get("succeeded")?,
        row.try_get("failed")?,
        row.try_get("total")?,
    ))
}
