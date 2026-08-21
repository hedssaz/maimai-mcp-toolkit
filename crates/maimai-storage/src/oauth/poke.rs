use secrecy::{ExposeSecret, SecretString};
use sqlx::Row;

use super::{
    NewOAuthToken, OAuthConfirmResult, OAuthContext, OAuthPendingPoke, model::is_valid_subject,
    token::upsert_token,
};
use crate::{StateStore, StorageError};

impl StateStore {
    pub async fn save_oauth_pending_poke(
        &self,
        pending: &OAuthPendingPoke,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO lxns_oauth_pending_pokes (
                subject, authorization_generation, adapter_id, group_id, bot_qq,
                access_token, refresh_token, token_type, scope, client_id,
                token_expires_at, created_at, expires_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (subject) DO UPDATE SET
                authorization_generation = excluded.authorization_generation,
                adapter_id = excluded.adapter_id,
                group_id = excluded.group_id,
                bot_qq = excluded.bot_qq,
                access_token = excluded.access_token,
                refresh_token = excluded.refresh_token,
                token_type = excluded.token_type,
                scope = excluded.scope,
                client_id = excluded.client_id,
                token_expires_at = excluded.token_expires_at,
                created_at = excluded.created_at,
                expires_at = excluded.expires_at
            "#,
        )
        .bind(&pending.subject)
        .bind(stored_integer(
            pending.authorization_generation,
            "oauth_pending.authorization_generation",
        )?)
        .bind(&pending.context.adapter_id)
        .bind(&pending.context.group_id)
        .bind(&pending.context.bot_qq)
        .bind(pending.token.access_token().expose_secret())
        .bind(pending.token.refresh_token().expose_secret())
        .bind(&pending.token.token_type)
        .bind(&pending.token.scope)
        .bind(&pending.token.client_id)
        .bind(pending.token.expires_at)
        .bind(pending.created_at)
        .bind(pending.expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn oauth_pending_poke(
        &self,
        subject: &str,
        now: i64,
    ) -> Result<Option<OAuthPendingPoke>, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let expired_generation: Option<i64> = sqlx::query_scalar(
            r#"
            DELETE FROM lxns_oauth_pending_pokes
            WHERE subject = ? AND expires_at <= ?
            RETURNING authorization_generation
            "#,
        )
        .bind(subject)
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(generation) = expired_generation {
            sqlx::query(
                "DELETE FROM lxns_oauth_authorizations WHERE subject = ? AND generation = ?",
            )
            .bind(subject)
            .bind(generation)
            .execute(&mut *transaction)
            .await?;
        }
        let pending = sqlx::query("SELECT * FROM lxns_oauth_pending_pokes WHERE subject = ?")
            .bind(subject)
            .fetch_optional(&mut *transaction)
            .await?;
        transaction.commit().await?;
        pending.map(pending_from_row).transpose()
    }

    pub async fn confirm_oauth_pending_poke(
        &self,
        subject: &str,
        context: &OAuthContext,
        now: i64,
    ) -> Result<OAuthConfirmResult, StorageError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT * FROM lxns_oauth_pending_pokes WHERE subject = ?")
            .bind(subject)
            .fetch_optional(&mut *transaction)
            .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(OAuthConfirmResult::NotFound);
        };
        if row.try_get::<String, _>("adapter_id")? != context.adapter_id
            || row.try_get::<String, _>("group_id")? != context.group_id
            || row.try_get::<String, _>("bot_qq")? != context.bot_qq
        {
            transaction.commit().await?;
            return Ok(OAuthConfirmResult::ContextMismatch);
        }
        let expires_at: i64 = row.try_get("expires_at")?;
        if expires_at <= now {
            let authorization_generation: i64 = row.try_get("authorization_generation")?;
            sqlx::query("DELETE FROM lxns_oauth_pending_pokes WHERE subject = ?")
                .bind(subject)
                .execute(&mut *transaction)
                .await?;
            sqlx::query(
                "DELETE FROM lxns_oauth_authorizations WHERE subject = ? AND generation = ?",
            )
            .bind(subject)
            .bind(authorization_generation)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(OAuthConfirmResult::Expired);
        }
        let pending = pending_from_row(row)?;
        let consumed = sqlx::query(
            r#"
            DELETE FROM lxns_oauth_authorizations
            WHERE subject = ? AND generation = ? AND status = 'exchanging'
            "#,
        )
        .bind(subject)
        .bind(stored_integer(
            pending.authorization_generation,
            "oauth_pending.authorization_generation",
        )?)
        .execute(&mut *transaction)
        .await?;
        if consumed.rows_affected() != 1 {
            sqlx::query("DELETE FROM lxns_oauth_pending_pokes WHERE subject = ?")
                .bind(subject)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            return Ok(OAuthConfirmResult::NotFound);
        }
        let token = upsert_token(&mut transaction, subject, &pending.token, now).await?;
        sqlx::query("DELETE FROM lxns_oauth_pending_pokes WHERE subject = ?")
            .bind(subject)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(OAuthConfirmResult::Confirmed(token))
    }
}

fn pending_from_row(row: sqlx::sqlite::SqliteRow) -> Result<OAuthPendingPoke, StorageError> {
    let subject: String = row.try_get("subject")?;
    if !is_valid_subject(&subject) {
        return Err(invalid("oauth_pending.subject", subject));
    }
    let generation: i64 = row.try_get("authorization_generation")?;
    Ok(OAuthPendingPoke {
        subject,
        authorization_generation: u64::try_from(generation).map_err(|_| {
            invalid(
                "oauth_pending.authorization_generation",
                generation.to_string(),
            )
        })?,
        context: OAuthContext {
            adapter_id: row.try_get("adapter_id")?,
            group_id: row.try_get("group_id")?,
            bot_qq: row.try_get("bot_qq")?,
        },
        token: NewOAuthToken::new(
            SecretString::from(row.try_get::<String, _>("access_token")?),
            SecretString::from(row.try_get::<String, _>("refresh_token")?),
            row.try_get("token_type")?,
            row.try_get("scope")?,
            row.try_get("client_id")?,
            row.try_get("token_expires_at")?,
        ),
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
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
