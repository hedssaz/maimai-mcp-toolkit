use maimai_core::{QqId, ScoreSource};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    StateStore,
    codec::{score_source_db, score_source_from_db},
};
use crate::StorageError;

impl StateStore {
    pub async fn set_score_source_preference(
        &self,
        qq: &QqId,
        source: ScoreSource,
    ) -> Result<(), StorageError> {
        let updated_at = OffsetDateTime::now_utc().format(&Rfc3339)?;
        sqlx::query(
            r#"
            INSERT INTO score_source_preferences (qq, score_source, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT (qq) DO UPDATE SET
                score_source = excluded.score_source,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(qq.as_str())
        .bind(score_source_db(source))
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn score_source_preference(
        &self,
        qq: &QqId,
    ) -> Result<Option<ScoreSource>, StorageError> {
        let value = sqlx::query_scalar::<_, String>(
            "SELECT score_source FROM score_source_preferences WHERE qq = ?",
        )
        .bind(qq.as_str())
        .fetch_optional(&self.pool)
        .await?;
        value.map(|value| score_source_from_db(&value)).transpose()
    }
}
