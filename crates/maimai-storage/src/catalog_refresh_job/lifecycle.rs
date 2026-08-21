use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{CatalogRefreshJobId, CatalogRefreshJobStore};
use crate::StorageError;

impl CatalogRefreshJobStore {
    pub async fn interrupt_catalog_refresh_job(
        &self,
        id: CatalogRefreshJobId,
        finished_at: OffsetDateTime,
    ) -> Result<bool, StorageError> {
        self.interrupt_jobs(Some(id), finished_at, "服务关闭").await
    }

    pub async fn interrupt_catalog_refresh_jobs(
        &self,
        finished_at: OffsetDateTime,
    ) -> Result<u64, StorageError> {
        self.interrupt_jobs(None, finished_at, "进程重启")
            .await
            .map(u64::from)
    }

    async fn interrupt_jobs(
        &self,
        id: Option<CatalogRefreshJobId>,
        finished_at: OffsetDateTime,
        reason: &str,
    ) -> Result<bool, StorageError> {
        let timestamp = finished_at.format(&Rfc3339)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let source_sql = match id {
            Some(_) => {
                "UPDATE catalog_refresh_job_sources SET status = 'failed', error_code = 'INTERRUPTED', error_message = 'the catalog process stopped before the job completed' WHERE job_id = ? AND status IN ('queued', 'pending')"
            }
            None => {
                "UPDATE catalog_refresh_job_sources SET status = 'failed', error_code = 'INTERRUPTED', error_message = 'the catalog process stopped before the job completed' WHERE job_id IN (SELECT id FROM catalog_refresh_jobs WHERE status IN ('queued', 'running')) AND status IN ('queued', 'pending')"
            }
        };
        let mut source_query = sqlx::query(source_sql);
        if let Some(id) = id {
            source_query = source_query.bind(id.value());
        }
        source_query.execute(&mut *transaction).await?;

        let job_sql = match id {
            Some(_) => {
                "UPDATE catalog_refresh_jobs SET status = 'interrupted', outcome = 'interrupted', finished_at = ?, message = ?, error_code = 'INTERRUPTED', error_message = 'the catalog process stopped before the job completed' WHERE id = ? AND status IN ('queued', 'running')"
            }
            None => {
                "UPDATE catalog_refresh_jobs SET status = 'interrupted', outcome = 'interrupted', finished_at = ?, message = ?, error_code = 'INTERRUPTED', error_message = 'the catalog process stopped before the job completed' WHERE status IN ('queued', 'running')"
            }
        };
        let mut job_query = sqlx::query(job_sql)
            .bind(timestamp)
            .bind(format!("刷新任务因{reason}而中断。"));
        if let Some(id) = id {
            job_query = job_query.bind(id.value());
        }
        let result = job_query.execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(result.rows_affected() > 0)
    }
}
