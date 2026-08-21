use maimai_core::{QqId, RatingBreakdown, ScoreSource};
use serde::{Deserialize, Serialize};
use sqlx::{Row, sqlite::SqliteRow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    B50CacheWriteOutcome, CachedB50Chart, CachedFitIndex, CachedPlayer, PlayerB50Snapshot,
    StateStore, StorageError,
};

#[derive(Serialize)]
struct StoredPayloadRef<'a> {
    player: &'a CachedPlayer,
    rating_breakdown: RatingBreakdown,
    fit_index: CachedFitIndex,
    charts: &'a [CachedB50Chart],
}

#[derive(Deserialize)]
struct StoredPayload {
    player: CachedPlayer,
    rating_breakdown: RatingBreakdown,
    fit_index: CachedFitIndex,
    charts: Vec<CachedB50Chart>,
}

impl StateStore {
    pub async fn replace_player_b50_snapshot(
        &self,
        snapshot: &PlayerB50Snapshot,
        existing_fresh_after: OffsetDateTime,
    ) -> Result<B50CacheWriteOutcome, StorageError> {
        let payload = serde_json::to_string(&StoredPayloadRef {
            player: snapshot.player(),
            rating_breakdown: snapshot.rating_breakdown(),
            fit_index: snapshot.fit_index(),
            charts: snapshot.charts(),
        })
        .map_err(|source| StorageError::EncodeJson {
            field: "player_b50_cache.payload_json",
            source,
        })?;
        let fetched_at = snapshot.fetched_at().format(&Rfc3339)?;
        let quality = snapshot.quality();
        let rating = snapshot.player().rating.map(i64::from);
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = sqlx::query(
            "SELECT score_source, fetched_at, metadata_quality, player_rating, payload_json \
             FROM player_b50_cache WHERE qq = ?",
        )
        .bind(snapshot.qq().as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(existing) = existing {
            let existing = decode_snapshot(snapshot.qq(), &existing)?;
            if existing.fetched_at() >= existing_fresh_after {
                let outcome = if existing.quality() > quality {
                    Some(B50CacheWriteOutcome::PreservedHigherQuality)
                } else if existing.quality() == quality
                    && existing.player().rating == snapshot.player().rating
                {
                    Some(B50CacheWriteOutcome::Unchanged)
                } else {
                    None
                };
                if let Some(outcome) = outcome {
                    transaction.rollback().await?;
                    return Ok(outcome);
                }
            }
        }
        sqlx::query(
            r#"
            INSERT INTO player_b50_cache (
                qq, score_source, fetched_at, metadata_quality, player_rating, payload_json
            ) VALUES (?, 'diving_fish', ?, ?, ?, ?)
            ON CONFLICT (qq) DO UPDATE SET
                score_source = excluded.score_source,
                fetched_at = excluded.fetched_at,
                metadata_quality = excluded.metadata_quality,
                player_rating = excluded.player_rating,
                payload_json = excluded.payload_json
            "#,
        )
        .bind(snapshot.qq().as_str())
        .bind(fetched_at)
        .bind(quality)
        .bind(rating)
        .bind(payload)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(B50CacheWriteOutcome::Written)
    }

    pub async fn player_b50_snapshot(
        &self,
        qq: &QqId,
        fresh_after: OffsetDateTime,
    ) -> Result<Option<PlayerB50Snapshot>, StorageError> {
        let row = sqlx::query(
            "SELECT score_source, fetched_at, metadata_quality, player_rating, payload_json \
             FROM player_b50_cache WHERE qq = ?",
        )
        .bind(qq.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let snapshot = decode_snapshot(qq, &row)?;
        if snapshot.fetched_at() < fresh_after {
            return Ok(None);
        }
        Ok(Some(snapshot))
    }
}

fn decode_snapshot(qq: &QqId, row: &SqliteRow) -> Result<PlayerB50Snapshot, StorageError> {
    let source: String = row.try_get("score_source")?;
    if source != "diving_fish" {
        return Err(invalid("player_b50_cache.score_source", source));
    }
    let fetched_at = parse_timestamp(row.try_get("fetched_at")?)?;
    let payload_text: String = row.try_get("payload_json")?;
    let payload: StoredPayload =
        serde_json::from_str(&payload_text).map_err(|source| StorageError::StoredJson {
            field: "player_b50_cache.payload_json",
            source,
        })?;
    let snapshot = PlayerB50Snapshot::new(
        qq.clone(),
        ScoreSource::DivingFish,
        fetched_at,
        payload.player,
        payload.rating_breakdown,
        payload.fit_index,
        payload.charts,
    )?;
    let stored_quality: i64 = row.try_get("metadata_quality")?;
    let stored_rating: Option<i64> = row.try_get("player_rating")?;
    let payload_rating = snapshot.player().rating.map(i64::from);
    if stored_quality != snapshot.quality() || stored_rating != payload_rating {
        return Err(invalid(
            "player_b50_cache.sidecar",
            "payload_mismatch".to_owned(),
        ));
    }
    Ok(snapshot)
}

fn parse_timestamp(value: String) -> Result<OffsetDateTime, StorageError> {
    OffsetDateTime::parse(&value, &Rfc3339).map_err(|source| StorageError::ParseTimestamp {
        field: "player_b50_cache.fetched_at",
        source,
    })
}

fn invalid(field: &'static str, value: String) -> StorageError {
    StorageError::InvalidStoredValue { field, value }
}
