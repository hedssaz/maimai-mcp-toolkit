use sqlx::{AssertSqlSafe, Row, SqlitePool};

use crate::StorageError;

const REQUIRED_COLUMNS: &[(&str, &str, &str)] = &[
    ("local_records", "payload_json", "TEXT"),
    ("local_records", "source_record_json", "TEXT"),
    ("local_profiles", "upper_profile_json", "TEXT"),
    ("local_profiles", "upper_profile_version", "TEXT"),
    ("local_profiles", "upper_profile_basic_updated_at", "TEXT"),
    (
        "local_profiles",
        "upper_profile_collection_updated_at",
        "TEXT",
    ),
    ("local_profiles", "upper_render_image_path", "TEXT"),
    ("local_profiles", "upper_render_signature", "TEXT"),
    ("local_profiles", "upper_render_updated_at", "TEXT"),
    (
        "player_records_v3",
        "achievement_kind",
        "TEXT CHECK (achievement_kind IN ('ranked', 'utage'))",
    ),
    ("player_records_v3", "achievement_units", "INTEGER"),
];

pub(super) async fn apply_column_upgrades(pool: &SqlitePool) -> Result<(), StorageError> {
    for (table, column, definition) in REQUIRED_COLUMNS {
        ensure_column(pool, table, column, definition).await?;
    }
    Ok(())
}

pub(super) async fn backfill_achievement_units(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        UPDATE player_records_v3
        SET achievement_units = CAST(
            ROUND(CAST(achievements AS NUMERIC) * 10000) AS INTEGER
        )
        WHERE achievement_units IS NULL AND achievements IS NOT NULL
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        UPDATE player_records_v3
        SET achievement_kind = CASE
            WHEN difficulty = 'utage' THEN 'utage'
            ELSE 'ranked'
        END
        WHERE achievement_kind IS NULL
          AND achievement_units IS NOT NULL
          AND (
              difficulty = 'utage'
              OR achievement_units BETWEEN 0 AND 1010000
          )
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_column(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), StorageError> {
    let pragma = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(AssertSqlSafe(pragma.as_str()))
        .fetch_all(pool)
        .await?;
    let found = rows
        .iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
        .any(|name| name == column);
    if !found {
        let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
        sqlx::query(AssertSqlSafe(alter.as_str()))
            .execute(pool)
            .await?;
    }
    Ok(())
}
