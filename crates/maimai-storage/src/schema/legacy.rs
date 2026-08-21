use sqlx::{AssertSqlSafe, Row, SqlitePool};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{LegacyImportReport, StorageError};

const LEGACY_IMPORT_NAME: &str = "local_records_to_player_records_v3_v2";

const ELIGIBLE_LEGACY_RECORD: &str = r#"
    UPPER(TRIM(type)) IN ('SD', 'ST', 'STANDARD', 'DX')
    AND level_index BETWEEN 0 AND 4
    AND (achievements IS NULL OR achievements BETWEEN 0 AND 101.0000)
    AND (
        (
            (
                LOWER(TRIM(COALESCE(source, ''))) LIKE 'sdgb_%'
                OR LOWER(TRIM(COALESCE(source, ''))) = 'official_cn'
            )
            AND (
                (
                    json_type(
                        CASE WHEN json_valid(source_record_json) THEN source_record_json END,
                        '$.musicId'
                    ) = 'integer'
                    AND json_extract(
                        CASE WHEN json_valid(source_record_json) THEN source_record_json END,
                        '$.musicId'
                    ) >= 0
                )
                OR (
                    json_type(
                        CASE WHEN json_valid(raw_json) THEN raw_json END,
                        '$.musicId'
                    ) = 'integer'
                    AND json_extract(
                        CASE WHEN json_valid(raw_json) THEN raw_json END,
                        '$.musicId'
                    ) >= 0
                )
            )
        )
        OR (
            LOWER(TRIM(COALESCE(source, ''))) IN (
                'diving_fish', 'divingfish', 'lxns', 'lxns_player'
            )
            AND song_id IS NOT NULL
            AND song_id >= 0
        )
    )
"#;

const LEGACY_NAMESPACE: &str = r#"
    CASE
        WHEN LOWER(TRIM(source)) LIKE 'sdgb_%'
            OR LOWER(TRIM(source)) = 'official_cn' THEN 'official_cn'
        WHEN LOWER(TRIM(source)) IN ('diving_fish', 'divingfish') THEN 'diving_fish'
        WHEN LOWER(TRIM(source)) IN ('lxns', 'lxns_player') THEN 'lxns'
    END
"#;

const LEGACY_SOURCE_VALUE: &str = r#"
    'numeric:' || CAST(
        CASE
            WHEN LOWER(TRIM(source)) LIKE 'sdgb_%'
                OR LOWER(TRIM(source)) = 'official_cn'
            THEN COALESCE(
                CASE
                    WHEN json_type(
                        CASE WHEN json_valid(source_record_json)
                            THEN source_record_json END,
                        '$.musicId'
                    ) = 'integer'
                    THEN json_extract(
                        CASE WHEN json_valid(source_record_json)
                            THEN source_record_json END,
                        '$.musicId'
                    )
                END,
                json_extract(
                    CASE WHEN json_valid(raw_json) THEN raw_json END,
                    '$.musicId'
                )
            )
            ELSE song_id
        END AS TEXT
    )
"#;

const LEGACY_GENERATION: &str = r#"
    CASE WHEN UPPER(TRIM(type)) IN ('SD', 'ST', 'STANDARD')
        THEN 'standard' ELSE 'deluxe' END
"#;

const LEGACY_DIFFICULTY: &str = r#"
    CASE level_index
        WHEN 0 THEN 'basic'
        WHEN 1 THEN 'advanced'
        WHEN 2 THEN 'expert'
        WHEN 3 THEN 'master'
        WHEN 4 THEN 're_master'
    END
"#;

pub(super) async fn import_legacy_records(
    pool: &SqlitePool,
) -> Result<LegacyImportReport, StorageError> {
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    let mut transaction = pool.begin().await?;
    let claim = sqlx::query(
        r#"
        INSERT OR IGNORE INTO maimai_storage_migrations (
            name, applied_at, imported_count, skipped_count
        ) VALUES (?, ?, 0, 0)
        "#,
    )
    .bind(LEGACY_IMPORT_NAME)
    .bind(&now)
    .execute(&mut *transaction)
    .await?;

    if claim.rows_affected() == 0 {
        let row = sqlx::query(
            "SELECT imported_count, skipped_count FROM maimai_storage_migrations WHERE name = ?",
        )
        .bind(LEGACY_IMPORT_NAME)
        .fetch_one(&mut *transaction)
        .await?;
        let report = LegacyImportReport {
            already_applied: true,
            imported: non_negative_count(row.try_get("imported_count")?, "imported_count")?,
            skipped: non_negative_count(row.try_get("skipped_count")?, "skipped_count")?,
        };
        transaction.commit().await?;
        return Ok(report);
    }

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM local_records")
        .fetch_one(&mut *transaction)
        .await?;
    let stable_partition = format!(
        "qq, {LEGACY_NAMESPACE}, {LEGACY_SOURCE_VALUE}, {LEGACY_GENERATION}, {LEGACY_DIFFICULTY}"
    );
    let imported_count_sql = format!(
        r#"
        SELECT COUNT(*)
        FROM (
            SELECT ROW_NUMBER() OVER (
                PARTITION BY {stable_partition}
                ORDER BY updated_at DESC, rowid DESC
            ) AS stable_rank
            FROM local_records
            WHERE {ELIGIBLE_LEGACY_RECORD}
        )
        WHERE stable_rank = 1
        "#,
    );
    let imported: i64 = sqlx::query_scalar(AssertSqlSafe(imported_count_sql.as_str()))
        .fetch_one(&mut *transaction)
        .await?;
    let import_sql = format!(
        r#"
        WITH ranked_legacy AS (
            SELECT local_records.*,
                   ROW_NUMBER() OVER (
                       PARTITION BY {stable_partition}
                       ORDER BY updated_at DESC, rowid DESC
                   ) AS stable_rank
            FROM local_records
            WHERE {ELIGIBLE_LEGACY_RECORD}
        )
        INSERT INTO player_records_v3 (
            qq, source_namespace, source_value, generation, difficulty,
            title, level, level_label, ds, achievements, achievement_kind, achievement_units,
            dx_score, fc, fs,
            rate, ra, version, is_new, score_source, source_detail, raw_json,
            payload_json, updated_at
        )
        SELECT
            qq,
            {LEGACY_NAMESPACE},
            {LEGACY_SOURCE_VALUE},
            {LEGACY_GENERATION},
            {LEGACY_DIFFICULTY},
            title, level, level_label, CAST(ds AS TEXT), CAST(achievements AS TEXT),
            CASE WHEN achievements IS NULL THEN NULL ELSE 'ranked' END,
            CASE WHEN achievements IS NULL THEN NULL
                ELSE CAST(ROUND(achievements * 10000) AS INTEGER) END,
            dx_score, fc, fs,
            rate, ra, version, CASE WHEN is_new = 0 THEN 0 ELSE 1 END,
            CASE
                WHEN LOWER(TRIM(source)) LIKE 'sdgb_%'
                    OR LOWER(TRIM(source)) = 'official_cn' THEN 'official_cn'
                WHEN LOWER(TRIM(source)) IN ('diving_fish', 'divingfish') THEN 'diving_fish'
                WHEN LOWER(TRIM(source)) IN ('lxns', 'lxns_player') THEN 'lxns'
            END,
            source,
            raw_json,
            COALESCE(payload_json, raw_json, '{{}}'),
            updated_at
        FROM ranked_legacy
        WHERE stable_rank = 1
        ON CONFLICT (qq, source_namespace, source_value, generation, difficulty)
        DO UPDATE SET
            title = excluded.title,
            level = excluded.level,
            level_label = excluded.level_label,
            ds = excluded.ds,
            achievements = excluded.achievements,
            achievement_kind = excluded.achievement_kind,
            achievement_units = excluded.achievement_units,
            dx_score = excluded.dx_score,
            fc = excluded.fc,
            fs = excluded.fs,
            rate = excluded.rate,
            ra = excluded.ra,
            version = excluded.version,
            is_new = excluded.is_new,
            score_source = excluded.score_source,
            source_detail = excluded.source_detail,
            raw_json = excluded.raw_json,
            payload_json = excluded.payload_json,
            updated_at = excluded.updated_at
        "#,
    );
    sqlx::query(AssertSqlSafe(import_sql.as_str()))
        .execute(&mut *transaction)
        .await?;

    let skipped = total.saturating_sub(imported);
    sqlx::query(
        r#"
        UPDATE maimai_storage_migrations
        SET applied_at = ?, imported_count = ?, skipped_count = ?
        WHERE name = ?
        "#,
    )
    .bind(&now)
    .bind(imported)
    .bind(skipped)
    .bind(LEGACY_IMPORT_NAME)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(LegacyImportReport {
        already_applied: false,
        imported: non_negative_count(imported, "imported_count")?,
        skipped: non_negative_count(skipped, "skipped_count")?,
    })
}

fn non_negative_count(value: i64, field: &'static str) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::InvalidStoredValue {
        field,
        value: value.to_string(),
    })
}
