use sqlx::SqlitePool;

use crate::StorageError;

const STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS catalog_refresh_jobs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        status TEXT NOT NULL CHECK (status IN (
            'queued', 'running', 'completed', 'failed', 'interrupted'
        )),
        outcome TEXT CHECK (outcome IS NULL OR outcome IN (
            'success', 'partial_failure', 'failed', 'interrupted'
        )),
        created_at TEXT NOT NULL,
        started_at TEXT,
        finished_at TEXT,
        message TEXT NOT NULL,
        error_code TEXT,
        error_message TEXT
    )
    "#,
    r#"
    CREATE UNIQUE INDEX IF NOT EXISTS idx_catalog_refresh_single_active
    ON catalog_refresh_jobs ((1))
    WHERE status IN ('queued', 'running')
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS catalog_refresh_job_sources (
        job_id INTEGER NOT NULL REFERENCES catalog_refresh_jobs(id) ON DELETE CASCADE,
        position INTEGER NOT NULL CHECK (position >= 0),
        source TEXT NOT NULL,
        due INTEGER NOT NULL DEFAULT 0 CHECK (due IN (0, 1)),
        status TEXT NOT NULL CHECK (status IN (
            'queued', 'pending', 'updated', 'not_modified', 'skipped', 'failed',
            'disk_updated_pending_reload'
        )),
        duration_millis INTEGER,
        error_code TEXT,
        error_message TEXT,
        PRIMARY KEY (job_id, source),
        UNIQUE (job_id, position)
    )
    "#,
];

pub(crate) async fn initialize(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in STATEMENTS {
        sqlx::query(*statement).execute(pool).await?;
    }
    Ok(())
}
