use sqlx::SqlitePool;

use crate::StorageError;

pub(crate) async fn initialize_player_cache_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS player_b50_cache (
            qq TEXT PRIMARY KEY,
            score_source TEXT NOT NULL CHECK (score_source = 'diving_fish'),
            fetched_at TEXT NOT NULL,
            metadata_quality INTEGER NOT NULL CHECK (metadata_quality BETWEEN 0 AND 2),
            player_rating INTEGER,
            payload_json TEXT NOT NULL
        ) STRICT
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}
