use maimai_core::{ChartKey, FullComboStatus, FullSyncStatus, QqId, SourceSongId};
use sqlx::{Row, sqlite::SqliteRow};

use super::{
    StateStore,
    codec::{
        chart_constant_db, chart_constant_from_db, decode_optional_json, difficulty_db,
        difficulty_from_db, encode_optional_json, generation_db, generation_from_db, namespace_db,
        namespace_from_db, play_achievement_db, play_achievement_from_db, qq_from_db,
        require_non_empty, score_source_db, score_source_from_db, source_value_db,
        source_value_from_db,
    },
};
use crate::{PlayerRecord, StorageError};

impl StateStore {
    pub async fn upsert_record(&self, record: &PlayerRecord) -> Result<(), StorageError> {
        require_non_empty(&record.title, "title")?;
        require_non_empty(&record.updated_at, "updated_at")?;
        let source_value = source_value_db(record.chart.song().value());
        let raw_json = encode_optional_json(record.raw.as_ref(), "player_records_v3.raw_json")?;
        let achievement = record
            .achievements
            .map(|value| play_achievement_db(value, record.chart.difficulty()))
            .transpose()?;
        let payload_json =
            serde_json::to_string(&record.payload).map_err(|source| StorageError::EncodeJson {
                field: "player_records_v3.payload_json",
                source,
            })?;

        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO player_records_v3 (
                qq, source_namespace, source_value, generation, difficulty,
                title, level, level_label, ds, achievements, achievement_kind, achievement_units,
                dx_score, fc, fs,
                rate, ra, version, is_new, score_source, source_detail, raw_json,
                payload_json, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        )
        .bind(record.qq.as_str())
        .bind(namespace_db(record.chart.song().namespace()))
        .bind(source_value)
        .bind(generation_db(record.chart.generation()))
        .bind(difficulty_db(record.chart.difficulty()))
        .bind(record.title.trim())
        .bind(&record.level)
        .bind(&record.level_label)
        .bind(record.ds.map(chart_constant_db))
        .bind(achievement.as_ref().map(|value| &value.decimal))
        .bind(achievement.as_ref().map(|value| value.kind))
        .bind(achievement.as_ref().map(|value| value.units))
        .bind(record.dx_score)
        .bind(record.fc.map(FullComboStatus::as_str))
        .bind(record.fs.map(FullSyncStatus::as_str))
        .bind(&record.rate)
        .bind(record.ra)
        .bind(&record.version)
        .bind(i64::from(record.is_new))
        .bind(score_source_db(record.score_source))
        .bind(&record.source_detail)
        .bind(raw_json)
        .bind(payload_json)
        .bind(&record.updated_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM player_score_snapshots WHERE qq = ?")
            .bind(record.qq.as_str())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn record(
        &self,
        qq: &QqId,
        chart: &ChartKey,
    ) -> Result<Option<PlayerRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT * FROM player_records_v3
            WHERE qq = ? AND source_namespace = ? AND source_value = ?
              AND generation = ? AND difficulty = ?
            "#,
        )
        .bind(qq.as_str())
        .bind(namespace_db(chart.song().namespace()))
        .bind(source_value_db(chart.song().value()))
        .bind(generation_db(chart.generation()))
        .bind(difficulty_db(chart.difficulty()))
        .fetch_optional(&self.pool)
        .await?;
        row.map(record_from_row).transpose()
    }

    pub async fn records_for_player(&self, qq: &QqId) -> Result<Vec<PlayerRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM player_records_v3
            WHERE qq = ?
            ORDER BY ra DESC, achievement_units DESC,
                     source_namespace, source_value, difficulty
            "#,
        )
        .bind(qq.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(record_from_row).collect()
    }
}

pub(super) fn record_from_row(row: SqliteRow) -> Result<PlayerRecord, StorageError> {
    let qq_text: String = row.try_get("qq")?;
    let namespace_text: String = row.try_get("source_namespace")?;
    let source_value_text: String = row.try_get("source_value")?;
    let generation_text: String = row.try_get("generation")?;
    let difficulty_text: String = row.try_get("difficulty")?;
    let score_source_text: String = row.try_get("score_source")?;
    let raw_json: Option<String> = row.try_get("raw_json")?;
    let payload_json: String = row.try_get("payload_json")?;
    let generation = generation_from_db(&generation_text)?;
    let difficulty = difficulty_from_db(&difficulty_text)?;
    let chart = ChartKey::new(
        SourceSongId::new(
            namespace_from_db(&namespace_text)?,
            source_value_from_db(&source_value_text)?,
        ),
        generation,
        difficulty,
    )
    .map_err(|_| StorageError::InvalidStoredValue {
        field: "player_records_v3.chart",
        value: format!("{namespace_text}/{source_value_text}/{generation_text}/{difficulty_text}"),
    })?;

    Ok(PlayerRecord {
        qq: qq_from_db(qq_text)?,
        chart,
        title: row.try_get("title")?,
        level: row.try_get("level")?,
        level_label: row.try_get("level_label")?,
        ds: row
            .try_get::<Option<String>, _>("ds")?
            .map(|value| chart_constant_from_db(&value))
            .transpose()?,
        achievements: play_achievement_from_db(
            row.try_get("achievement_kind")?,
            row.try_get("achievement_units")?,
            difficulty,
        )?,
        dx_score: row.try_get("dx_score")?,
        fc: marker_from_db(row.try_get("fc")?, "player_records_v3.fc")?,
        fs: marker_from_db(row.try_get("fs")?, "player_records_v3.fs")?,
        rate: row.try_get("rate")?,
        ra: row.try_get("ra")?,
        version: row.try_get("version")?,
        is_new: row.try_get::<i64, _>("is_new")? != 0,
        score_source: score_source_from_db(&score_source_text)?,
        source_detail: row.try_get("source_detail")?,
        raw: decode_optional_json(raw_json, "player_records_v3.raw_json")?,
        payload: serde_json::from_str(&payload_json).map_err(|source| {
            StorageError::StoredJson {
                field: "player_records_v3.payload_json",
                source,
            }
        })?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn marker_from_db<T>(value: Option<String>, field: &'static str) -> Result<Option<T>, StorageError>
where
    T: std::str::FromStr,
{
    value
        .map(|value| {
            value
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue { field, value })
        })
        .transpose()
}
