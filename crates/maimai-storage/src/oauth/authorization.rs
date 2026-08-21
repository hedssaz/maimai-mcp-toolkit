use secrecy::{ExposeSecret, SecretString};
use sqlx::{Row, sqlite::SqliteRow};

use super::{
    AuthorizationClaim, AuthorizationClaimResult, NewOAuthAuthorization, OAuthAuthorization,
    OAuthContext, model::is_valid_subject,
};
use crate::{StateStore, StorageError};

impl StateStore {
    pub async fn save_oauth_authorization(
        &self,
        authorization: &NewOAuthAuthorization,
    ) -> Result<OAuthAuthorization, StorageError> {
        let context = authorization.context.as_ref();
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            INSERT INTO lxns_oauth_authorizations (
                subject, generation, state, code_verifier, adapter_id, group_id, bot_qq,
                status, created_at, expires_at
            ) VALUES (?, 1, ?, ?, ?, ?, ?, 'pending', ?, ?)
            ON CONFLICT (subject) DO UPDATE SET
                generation = lxns_oauth_authorizations.generation + 1,
                state = excluded.state,
                code_verifier = excluded.code_verifier,
                adapter_id = excluded.adapter_id,
                group_id = excluded.group_id,
                bot_qq = excluded.bot_qq,
                status = 'pending',
                created_at = excluded.created_at,
                expires_at = excluded.expires_at
            RETURNING *
            "#,
        )
        .bind(&authorization.subject)
        .bind(authorization.state().expose_secret())
        .bind(authorization.code_verifier().expose_secret())
        .bind(context.map(|context| context.adapter_id.as_str()))
        .bind(context.map(|context| context.group_id.as_str()))
        .bind(context.map(|context| context.bot_qq.as_str()))
        .bind(authorization.created_at)
        .bind(authorization.expires_at)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM lxns_oauth_pending_pokes WHERE subject = ?")
            .bind(&authorization.subject)
            .execute(&mut *transaction)
            .await?;
        let authorization = authorization_from_row(row)?;
        transaction.commit().await?;
        Ok(authorization)
    }

    pub async fn claim_oauth_authorization(
        &self,
        subject: &str,
        explicit_state: Option<&SecretString>,
        callback_state: Option<&SecretString>,
        context: Option<&OAuthContext>,
        now: i64,
    ) -> Result<AuthorizationClaimResult, StorageError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT * FROM lxns_oauth_authorizations WHERE subject = ?")
            .bind(subject)
            .fetch_optional(&mut *transaction)
            .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(AuthorizationClaimResult::NotFound);
        };
        let stored_state: String = row.try_get("state")?;
        let explicit_mismatch =
            explicit_state.is_some_and(|state| state.expose_secret() != stored_state);
        let callback_mismatch =
            callback_state.is_some_and(|state| state.expose_secret() != stored_state);
        if explicit_mismatch || callback_mismatch {
            transaction.commit().await?;
            return Ok(AuthorizationClaimResult::StateMismatch);
        }
        let authorization = authorization_from_row(row)?;
        if authorization.context.as_ref() != context {
            transaction.commit().await?;
            return Ok(AuthorizationClaimResult::ContextMismatch);
        }
        if authorization.expires_at <= now {
            sqlx::query("DELETE FROM lxns_oauth_authorizations WHERE subject = ?")
                .bind(subject)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            return Ok(AuthorizationClaimResult::Expired);
        }
        let status: String =
            sqlx::query_scalar("SELECT status FROM lxns_oauth_authorizations WHERE subject = ?")
                .bind(subject)
                .fetch_one(&mut *transaction)
                .await?;
        if status == "exchanging" {
            transaction.commit().await?;
            return Ok(AuthorizationClaimResult::Busy);
        }
        if status != "pending" {
            return Err(invalid("oauth_authorization.status", status));
        }
        let generation =
            stored_integer(authorization.generation, "oauth_authorization.generation")?;
        let updated = sqlx::query(
            r#"
            UPDATE lxns_oauth_authorizations SET status = 'exchanging'
            WHERE subject = ? AND generation = ? AND status = 'pending'
            "#,
        )
        .bind(subject)
        .bind(generation)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            transaction.commit().await?;
            return Ok(AuthorizationClaimResult::Busy);
        }
        transaction.commit().await?;
        Ok(AuthorizationClaimResult::Claimed(AuthorizationClaim {
            authorization,
        }))
    }

    pub async fn release_oauth_authorization_claim(
        &self,
        subject: &str,
        generation: u64,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE lxns_oauth_authorizations SET status = 'pending'
            WHERE subject = ? AND generation = ? AND status = 'exchanging'
            "#,
        )
        .bind(subject)
        .bind(stored_integer(
            generation,
            "oauth_authorization.generation",
        )?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn oauth_authorization(
        &self,
        subject: &str,
        now: i64,
    ) -> Result<Option<OAuthAuthorization>, StorageError> {
        sqlx::query("DELETE FROM lxns_oauth_authorizations WHERE subject = ? AND expires_at <= ?")
            .bind(subject)
            .bind(now)
            .execute(&self.pool)
            .await?;
        sqlx::query("SELECT * FROM lxns_oauth_authorizations WHERE subject = ?")
            .bind(subject)
            .fetch_optional(&self.pool)
            .await?
            .map(authorization_from_row)
            .transpose()
    }
}

fn authorization_from_row(row: SqliteRow) -> Result<OAuthAuthorization, StorageError> {
    let subject: String = row.try_get("subject")?;
    validate_stored_subject(&subject)?;
    let generation: i64 = row.try_get("generation")?;
    let adapter_id: Option<String> = row.try_get("adapter_id")?;
    let group_id: Option<String> = row.try_get("group_id")?;
    let bot_qq: Option<String> = row.try_get("bot_qq")?;
    let context = match (adapter_id, group_id, bot_qq) {
        (Some(adapter_id), Some(group_id), Some(bot_qq)) => Some(OAuthContext {
            adapter_id,
            group_id,
            bot_qq,
        }),
        (None, None, None) => None,
        _ => return Err(invalid("oauth_authorization.context", "partial")),
    };
    Ok(OAuthAuthorization {
        subject,
        generation: u64::try_from(generation)
            .map_err(|_| invalid("oauth_authorization.generation", generation.to_string()))?,
        code_verifier: SecretString::from(row.try_get::<String, _>("code_verifier")?),
        context,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

fn validate_stored_subject(subject: &str) -> Result<(), StorageError> {
    if !is_valid_subject(subject) {
        return Err(invalid("oauth_authorization.subject", subject));
    }
    Ok(())
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
