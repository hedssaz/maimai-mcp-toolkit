use sqlx::SqlitePool;

use crate::StorageError;

pub(crate) async fn initialize_diving_fish_credentials_schema(
    pool: &SqlitePool,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS diving_fish_developer_token (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            developer_token TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}
