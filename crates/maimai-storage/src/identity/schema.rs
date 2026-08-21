use sqlx::{Row, SqlitePool};

use crate::StorageError;

const IDENTITY_TABLES: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS identity_users (
        qq TEXT PRIMARY KEY,
        is_friend INTEGER NOT NULL DEFAULT 0 CHECK (is_friend IN (0, 1)),
        qq_nickname TEXT,
        friend_nickname TEXT,
        waterfish_nickname TEXT,
        waterfish_username TEXT,
        waterfish_rating INTEGER,
        waterfish_updated_at TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS identity_groups (
        group_id TEXT PRIMARY KEY,
        group_name TEXT,
        member_count INTEGER,
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS identity_members (
        group_id TEXT NOT NULL REFERENCES identity_groups(group_id) ON DELETE CASCADE,
        qq TEXT NOT NULL REFERENCES identity_users(qq) ON DELETE CASCADE,
        group_nickname TEXT NOT NULL,
        card TEXT,
        nickname TEXT,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (group_id, qq)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_identity_members_qq ON identity_members (qq)",
    r#"
    CREATE TABLE IF NOT EXISTS identity_metadata (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        fetched_at TEXT,
        updated_at TEXT NOT NULL,
        generation INTEGER NOT NULL,
        friend_count INTEGER NOT NULL,
        group_count INTEGER NOT NULL,
        group_member_rows INTEGER NOT NULL,
        unique_users INTEGER NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS identity_refresh_job (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        generation INTEGER NOT NULL,
        status TEXT NOT NULL CHECK (status IN (
            'running', 'completed', 'failed', 'interrupted'
        )),
        started_at TEXT NOT NULL,
        finished_at TEXT,
        refresh_reason TEXT NOT NULL CHECK (refresh_reason IN (
            'force_refresh', 'stale_or_missing', 'auto_daily'
        )),
        message TEXT NOT NULL,
        processed_groups INTEGER NOT NULL DEFAULT 0,
        total_groups INTEGER,
        friend_count INTEGER,
        current_group_id TEXT,
        current_group_name TEXT,
        unique_users INTEGER,
        stats_friend_count INTEGER,
        stats_group_count INTEGER,
        stats_group_member_rows INTEGER,
        stats_unique_users INTEGER,
        error_code TEXT,
        error_message TEXT,
        error_status INTEGER,
        error_body TEXT
    )
    "#,
];

pub(crate) async fn initialize_identity_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in IDENTITY_TABLES {
        sqlx::query(*statement).execute(pool).await?;
    }
    ensure_nullable_fetched_at(pool).await?;
    Ok(())
}

async fn ensure_nullable_fetched_at(pool: &SqlitePool) -> Result<(), StorageError> {
    let columns = sqlx::query("PRAGMA table_info(identity_metadata)")
        .fetch_all(pool)
        .await?;
    let fetched_at_not_null = columns.iter().any(|row| {
        row.try_get::<String, _>("name").ok().as_deref() == Some("fetched_at")
            && row.try_get::<i64, _>("notnull").ok() == Some(1)
    });
    if !fetched_at_not_null {
        return Ok(());
    }

    let mut transaction = pool.begin().await?;
    sqlx::query("ALTER TABLE identity_metadata RENAME TO identity_metadata_legacy")
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        r#"
        CREATE TABLE identity_metadata (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            fetched_at TEXT,
            updated_at TEXT NOT NULL,
            generation INTEGER NOT NULL,
            friend_count INTEGER NOT NULL,
            group_count INTEGER NOT NULL,
            group_member_rows INTEGER NOT NULL,
            unique_users INTEGER NOT NULL
        )
        "#,
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO identity_metadata
        SELECT singleton, fetched_at, updated_at, generation,
               friend_count, group_count, group_member_rows, unique_users
        FROM identity_metadata_legacy
        "#,
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DROP TABLE identity_metadata_legacy")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}
