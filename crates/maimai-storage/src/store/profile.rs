use maimai_core::QqId;
use sqlx::{Row, sqlite::SqliteRow};

use super::{
    StateStore,
    codec::{
        decode_optional_json, encode_optional_json, qq_from_db, require_non_empty, score_source_db,
        score_source_from_legacy_profile,
    },
};
use crate::{PlayerProfile, StorageError};

impl StateStore {
    pub async fn upsert_profile(&self, profile: &PlayerProfile) -> Result<(), StorageError> {
        require_non_empty(&profile.updated_at, "updated_at")?;
        let raw_json = encode_optional_json(profile.raw.as_ref(), "local_profiles.raw_json")?;
        let source = profile
            .source_detail
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| profile.score_source.map(score_source_db).map(str::to_owned));

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
        .bind(source)
        .bind(raw_json)
        .bind(&profile.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn profile(&self, qq: &QqId) -> Result<Option<PlayerProfile>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT qq, nickname, player_rating, player_old_rating, player_new_rating,
                   source, raw_json, updated_at
            FROM local_profiles
            WHERE qq = ?
            "#,
        )
        .bind(qq.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(profile_from_row).transpose()
    }
}

pub(super) fn profile_from_row(row: SqliteRow) -> Result<PlayerProfile, StorageError> {
    let qq_text: String = row.try_get("qq")?;
    let source_detail: Option<String> = row.try_get("source")?;
    let score_source = source_detail
        .as_deref()
        .and_then(score_source_from_legacy_profile);
    Ok(PlayerProfile {
        qq: qq_from_db(qq_text)?,
        nickname: row.try_get("nickname")?,
        player_rating: row.try_get("player_rating")?,
        player_old_rating: row.try_get("player_old_rating")?,
        player_new_rating: row.try_get("player_new_rating")?,
        score_source,
        source_detail,
        raw: decode_optional_json(row.try_get("raw_json")?, "local_profiles.raw_json")?,
        updated_at: row.try_get("updated_at")?,
    })
}
