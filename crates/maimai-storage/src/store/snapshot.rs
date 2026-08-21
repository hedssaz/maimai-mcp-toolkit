use std::collections::HashSet;

use sqlx::{QueryBuilder, Row, Sqlite};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    StateStore,
    codec::{
        DbAchievement, chart_constant_db, difficulty_db, encode_optional_json, generation_db,
        namespace_db, play_achievement_db, require_non_empty, score_source_db,
        score_source_from_db, source_value_db,
    },
    profile::profile_from_row,
    record::record_from_row,
};
use crate::{
    FullScoreSnapshot, FullScoreSnapshotWriteOutcome, PlayerProfile, PlayerRecord, StorageError,
};

const RECORD_COLUMNS: usize = 24;
const SQLITE_BIND_LIMIT: usize = 32_000;
const RECORDS_PER_INSERT: usize = SQLITE_BIND_LIMIT / RECORD_COLUMNS;

struct EncodedRecord<'a> {
    record: &'a PlayerRecord,
    source_value: String,
    constant: Option<String>,
    achievement: Option<DbAchievement>,
    raw_json: Option<String>,
    payload_json: String,
}

impl StateStore {
    pub async fn replace_player_score_snapshot(
        &self,
        profile: &PlayerProfile,
        records: &[PlayerRecord],
    ) -> Result<FullScoreSnapshotWriteOutcome, StorageError> {
        let (source, fetched_at) = validate_snapshot(profile, records)?;
        let profile_raw = encode_optional_json(profile.raw.as_ref(), "local_profiles.raw_json")?;
        let profile_source = profile
            .source_detail
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| profile.score_source.map(score_source_db).map(str::to_owned));
        let encoded = records
            .iter()
            .map(encode_record)
            .collect::<Result<Vec<_>, _>>()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing_at = sqlx::query_scalar::<_, String>(
            "SELECT fetched_at FROM player_score_snapshots WHERE qq = ?",
        )
        .bind(profile.qq.as_str())
        .fetch_optional(&mut *transaction)
        .await?
        .map(|value| parse_timestamp(value, "player_score_snapshots.fetched_at"))
        .transpose()?;
        if existing_at.is_some_and(|existing| fetched_at < existing) {
            transaction.rollback().await?;
            return Ok(FullScoreSnapshotWriteOutcome::StaleIgnored);
        }
        sqlx::query(
            r#"
            INSERT INTO local_profiles (
                qq, nickname, player_rating, player_old_rating, player_new_rating,
                source, raw_json, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (qq) DO UPDATE SET
                nickname = excluded.nickname,
                player_rating = excluded.player_rating,
                player_old_rating = excluded.player_old_rating,
                player_new_rating = excluded.player_new_rating,
                source = excluded.source,
                raw_json = excluded.raw_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(profile.qq.as_str())
        .bind(&profile.nickname)
        .bind(profile.player_rating)
        .bind(profile.player_old_rating)
        .bind(profile.player_new_rating)
        .bind(profile_source)
        .bind(profile_raw)
        .bind(&profile.updated_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM player_records_v3 WHERE qq = ?")
            .bind(profile.qq.as_str())
            .execute(&mut *transaction)
            .await?;
        for chunk in encoded.chunks(RECORDS_PER_INSERT) {
            let mut query = QueryBuilder::<Sqlite>::new(
                r#"
                INSERT INTO player_records_v3 (
                    qq, source_namespace, source_value, generation, difficulty,
                    title, level, level_label, ds, achievements, achievement_kind,
                    achievement_units,
                    dx_score, fc, fs, rate, ra, version, is_new, score_source,
                    source_detail, raw_json, payload_json, updated_at
                )
                "#,
            );
            query.push_values(chunk, |mut row, encoded| {
                let record = encoded.record;
                row.push_bind(record.qq.as_str())
                    .push_bind(namespace_db(record.chart.song().namespace()))
                    .push_bind(&encoded.source_value)
                    .push_bind(generation_db(record.chart.generation()))
                    .push_bind(difficulty_db(record.chart.difficulty()))
                    .push_bind(record.title.trim())
                    .push_bind(&record.level)
                    .push_bind(&record.level_label)
                    .push_bind(&encoded.constant)
                    .push_bind(encoded.achievement.as_ref().map(|value| &value.decimal))
                    .push_bind(encoded.achievement.as_ref().map(|value| value.kind))
                    .push_bind(encoded.achievement.as_ref().map(|value| value.units))
                    .push_bind(record.dx_score)
                    .push_bind(record.fc.map(maimai_core::FullComboStatus::as_str))
                    .push_bind(record.fs.map(maimai_core::FullSyncStatus::as_str))
                    .push_bind(&record.rate)
                    .push_bind(record.ra)
                    .push_bind(&record.version)
                    .push_bind(i64::from(record.is_new))
                    .push_bind(score_source_db(record.score_source))
                    .push_bind(&record.source_detail)
                    .push_bind(&encoded.raw_json)
                    .push_bind(&encoded.payload_json)
                    .push_bind(&record.updated_at);
            });
            query.build().execute(&mut *transaction).await?;
        }
        sqlx::query(
            r#"
            INSERT INTO player_score_snapshots (qq, score_source, fetched_at, record_count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (qq) DO UPDATE SET
                score_source = excluded.score_source,
                fetched_at = excluded.fetched_at,
                record_count = excluded.record_count
            "#,
        )
        .bind(profile.qq.as_str())
        .bind(score_source_db(source))
        .bind(fetched_at.format(&Rfc3339)?)
        .bind(
            i64::try_from(records.len())
                .map_err(|_| invalid("player_score_snapshot.record_count", "overflow"))?,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(FullScoreSnapshotWriteOutcome::Written)
    }

    pub async fn full_score_snapshot(
        &self,
        qq: &maimai_core::QqId,
        source: maimai_core::ScoreSource,
        fresh_after: OffsetDateTime,
    ) -> Result<Option<FullScoreSnapshot>, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let marker = sqlx::query(
            "SELECT score_source, fetched_at, record_count FROM player_score_snapshots WHERE qq = ?",
        )
        .bind(qq.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(marker) = marker else {
            transaction.rollback().await?;
            return Ok(None);
        };
        let stored_source = score_source_from_db(&marker.try_get::<String, _>("score_source")?)?;
        let fetched_at = parse_timestamp(
            marker.try_get("fetched_at")?,
            "player_score_snapshots.fetched_at",
        )?;
        if stored_source != source || fetched_at < fresh_after {
            transaction.rollback().await?;
            return Ok(None);
        }
        let expected_count = usize::try_from(marker.try_get::<i64, _>("record_count")?)
            .map_err(|_| invalid("player_score_snapshots.record_count", "out_of_range"))?;
        let profile = sqlx::query(
            r#"SELECT qq, nickname, player_rating, player_old_rating, player_new_rating,
                      source, raw_json, updated_at FROM local_profiles WHERE qq = ?"#,
        )
        .bind(qq.as_str())
        .fetch_optional(&mut *transaction)
        .await?
        .map(profile_from_row)
        .transpose()?;
        let rows = sqlx::query(
            r#"SELECT * FROM player_records_v3 WHERE qq = ?
               ORDER BY ra DESC, achievement_units DESC,
                        source_namespace, source_value, difficulty"#,
        )
        .bind(qq.as_str())
        .fetch_all(&mut *transaction)
        .await?;
        let records = rows
            .into_iter()
            .map(record_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        transaction.rollback().await?;
        let Some(profile) = profile else {
            return Ok(None);
        };
        if records.len() != expected_count
            || records
                .iter()
                .any(|record| record.qq != *qq || record.score_source != source)
            || parse_timestamp(profile.updated_at.clone(), "local_profiles.updated_at")?
                != fetched_at
        {
            return Ok(None);
        }
        for record in &records {
            if parse_timestamp(record.updated_at.clone(), "player_records_v3.updated_at")?
                != fetched_at
            {
                return Ok(None);
            }
        }
        Ok(Some(FullScoreSnapshot::new(
            source, fetched_at, profile, records,
        )))
    }
}

fn validate_snapshot(
    profile: &PlayerProfile,
    records: &[PlayerRecord],
) -> Result<(maimai_core::ScoreSource, OffsetDateTime), StorageError> {
    require_non_empty(&profile.updated_at, "updated_at")?;
    let source = profile
        .score_source
        .ok_or_else(|| invalid("player_score_snapshot.source", "profile_source_missing"))?;
    let fetched_at = parse_timestamp(
        profile.updated_at.clone(),
        "player_score_snapshot.fetched_at",
    )?;
    let mut keys = HashSet::new();
    for record in records {
        require_non_empty(&record.title, "title")?;
        require_non_empty(&record.updated_at, "updated_at")?;
        if record.qq != profile.qq {
            return Err(StorageError::InvalidStoredValue {
                field: "player_score_snapshot.qq",
                value: "mismatch".to_owned(),
            });
        }
        if record.score_source != source || record.updated_at != profile.updated_at {
            return Err(invalid(
                "player_score_snapshot.source",
                "mixed_or_stale_rows",
            ));
        }
        if !keys.insert(record.chart.clone()) {
            return Err(StorageError::InvalidStoredValue {
                field: "player_score_snapshot.chart",
                value: "duplicate".to_owned(),
            });
        }
    }
    Ok((source, fetched_at))
}

fn encode_record(record: &PlayerRecord) -> Result<EncodedRecord<'_>, StorageError> {
    Ok(EncodedRecord {
        record,
        source_value: source_value_db(record.chart.song().value()),
        constant: record.ds.map(chart_constant_db),
        achievement: record
            .achievements
            .map(|value| play_achievement_db(value, record.chart.difficulty()))
            .transpose()?,
        raw_json: encode_optional_json(record.raw.as_ref(), "player_records_v3.raw_json")?,
        payload_json: serde_json::to_string(&record.payload).map_err(|source| {
            StorageError::EncodeJson {
                field: "player_records_v3.payload_json",
                source,
            }
        })?,
    })
}

fn parse_timestamp(value: String, field: &'static str) -> Result<OffsetDateTime, StorageError> {
    OffsetDateTime::parse(&value, &Rfc3339)
        .map_err(|source| StorageError::ParseTimestamp { field, source })
}

fn invalid(field: &'static str, value: &str) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.to_owned(),
    }
}
