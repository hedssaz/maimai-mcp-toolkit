use secrecy::{ExposeSecret, SecretString};
use sqlx::Row;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{DeveloperTokenMetadata, DivingFishDeveloperToken};
use crate::{StateStore, StorageError};

impl StateStore {
    pub async fn set_diving_fish_developer_token(
        &self,
        token: &SecretString,
        updated_at: OffsetDateTime,
    ) -> Result<DeveloperTokenMetadata, StorageError> {
        let updated_at_text = updated_at.format(&Rfc3339)?;
        sqlx::query(
            r#"
            INSERT INTO diving_fish_developer_token (
                singleton, developer_token, updated_at
            ) VALUES (1, ?, ?)
            ON CONFLICT (singleton) DO UPDATE SET
                developer_token = excluded.developer_token,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(token.expose_secret())
        .bind(updated_at_text)
        .execute(&self.pool)
        .await?;
        Ok(DeveloperTokenMetadata { updated_at })
    }

    pub async fn diving_fish_developer_token(
        &self,
    ) -> Result<Option<DivingFishDeveloperToken>, StorageError> {
        let row = sqlx::query(
            "SELECT developer_token, updated_at FROM diving_fish_developer_token WHERE singleton = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let updated_at_text: String = row.try_get("updated_at")?;
            Ok(DivingFishDeveloperToken {
                token: SecretString::from(row.try_get::<String, _>("developer_token")?),
                updated_at: parse_updated_at(updated_at_text)?,
            })
        })
        .transpose()
    }

    pub async fn diving_fish_developer_token_metadata(
        &self,
    ) -> Result<Option<DeveloperTokenMetadata>, StorageError> {
        let updated_at = sqlx::query_scalar::<_, String>(
            "SELECT updated_at FROM diving_fish_developer_token WHERE singleton = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        updated_at
            .map(|value| {
                Ok(DeveloperTokenMetadata {
                    updated_at: parse_updated_at(value)?,
                })
            })
            .transpose()
    }

    pub async fn clear_diving_fish_developer_token(&self) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM diving_fish_developer_token WHERE singleton = 1")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }
}

fn parse_updated_at(value: String) -> Result<OffsetDateTime, StorageError> {
    OffsetDateTime::parse(&value, &Rfc3339).map_err(|_| StorageError::InvalidStoredValue {
        field: "diving_fish_developer_token.updated_at",
        value,
    })
}
