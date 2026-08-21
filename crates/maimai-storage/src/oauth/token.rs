use secrecy::{ExposeSecret, SecretString};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use super::{NewOAuthToken, OAuthCasResult, OAuthTokenRecord, model::is_valid_subject};
use crate::{StateStore, StorageError};

impl StateStore {
    pub async fn commit_oauth_authorization(
        &self,
        subject: &str,
        authorization_generation: u64,
        token: &NewOAuthToken,
        now: i64,
    ) -> Result<Option<OAuthTokenRecord>, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let deleted = sqlx::query(
            r#"
            DELETE FROM lxns_oauth_authorizations
            WHERE subject = ? AND generation = ? AND status = 'exchanging'
            "#,
        )
        .bind(subject)
        .bind(stored_integer(
            authorization_generation,
            "oauth_authorization.generation",
        )?)
        .execute(&mut *transaction)
        .await?;
        if deleted.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(None);
        }
        let record = upsert_token(&mut transaction, subject, token, now).await?;
        sqlx::query("DELETE FROM lxns_oauth_pending_pokes WHERE subject = ?")
            .bind(subject)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(Some(record))
    }

    pub async fn oauth_token(
        &self,
        subject: &str,
    ) -> Result<Option<OAuthTokenRecord>, StorageError> {
        sqlx::query("SELECT * FROM lxns_oauth_tokens WHERE subject = ?")
            .bind(subject)
            .fetch_optional(&self.pool)
            .await?
            .map(token_from_row)
            .transpose()
    }

    pub async fn compare_and_swap_oauth_token(
        &self,
        subject: &str,
        expected_generation: u64,
        token: &NewOAuthToken,
        now: i64,
    ) -> Result<OAuthCasResult, StorageError> {
        let row = sqlx::query(
            r#"
            UPDATE lxns_oauth_tokens SET
                generation = generation + 1,
                access_token = ?, refresh_token = ?, token_type = ?, scope = ?,
                client_id = ?, expires_at = ?, updated_at = ?
            WHERE subject = ? AND generation = ?
            RETURNING *
            "#,
        )
        .bind(token.access_token().expose_secret())
        .bind(token.refresh_token().expose_secret())
        .bind(&token.token_type)
        .bind(&token.scope)
        .bind(&token.client_id)
        .bind(token.expires_at)
        .bind(now)
        .bind(subject)
        .bind(stored_integer(
            expected_generation,
            "oauth_token.generation",
        )?)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            return Ok(OAuthCasResult::Stored(token_from_row(row)?));
        }
        Ok(match self.oauth_token(subject).await? {
            Some(record) => OAuthCasResult::Conflict(record),
            None => OAuthCasResult::Missing,
        })
    }

    pub async fn unbind_oauth(&self, subject: &str) -> Result<bool, StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM lxns_oauth_authorizations WHERE subject = ?")
            .bind(subject)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM lxns_oauth_pending_pokes WHERE subject = ?")
            .bind(subject)
            .execute(&mut *transaction)
            .await?;
        let deleted = sqlx::query("DELETE FROM lxns_oauth_tokens WHERE subject = ?")
            .bind(subject)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(deleted.rows_affected() == 1)
    }
}

pub(super) async fn upsert_token(
    transaction: &mut Transaction<'_, Sqlite>,
    subject: &str,
    token: &NewOAuthToken,
    now: i64,
) -> Result<OAuthTokenRecord, StorageError> {
    let row = sqlx::query(
        r#"
        INSERT INTO lxns_oauth_tokens (
            subject, generation, access_token, refresh_token, token_type, scope,
            client_id, expires_at, bound_at, updated_at
        ) VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (subject) DO UPDATE SET
            generation = lxns_oauth_tokens.generation + 1,
            access_token = excluded.access_token,
            refresh_token = excluded.refresh_token,
            token_type = excluded.token_type,
            scope = excluded.scope,
            client_id = excluded.client_id,
            expires_at = excluded.expires_at,
            updated_at = excluded.updated_at
        RETURNING *
        "#,
    )
    .bind(subject)
    .bind(token.access_token().expose_secret())
    .bind(token.refresh_token().expose_secret())
    .bind(&token.token_type)
    .bind(&token.scope)
    .bind(&token.client_id)
    .bind(token.expires_at)
    .bind(now)
    .bind(now)
    .fetch_one(&mut **transaction)
    .await?;
    token_from_row(row)
}

pub(super) fn token_from_row(row: SqliteRow) -> Result<OAuthTokenRecord, StorageError> {
    let subject: String = row.try_get("subject")?;
    if !is_valid_subject(&subject) {
        return Err(invalid("oauth_token.subject", subject));
    }
    let generation: i64 = row.try_get("generation")?;
    Ok(OAuthTokenRecord {
        subject,
        generation: u64::try_from(generation)
            .map_err(|_| invalid("oauth_token.generation", generation.to_string()))?,
        access_token: SecretString::from(row.try_get::<String, _>("access_token")?),
        refresh_token: SecretString::from(row.try_get::<String, _>("refresh_token")?),
        token_type: row.try_get("token_type")?,
        scope: row.try_get("scope")?,
        client_id: row.try_get("client_id")?,
        expires_at: row.try_get("expires_at")?,
        bound_at: row.try_get("bound_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn stored_integer(value: u64, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

fn invalid(field: &'static str, value: impl Into<String>) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.into(),
    }
}
