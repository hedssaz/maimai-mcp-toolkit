use std::{error::Error, sync::Arc};

use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use tempfile::TempDir;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::Barrier;

use super::{
    AuthorizationClaimResult, NewOAuthAuthorization, NewOAuthToken, OAuthConfirmResult,
    OAuthContext, OAuthPendingPoke,
};
use crate::StateStore;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn context() -> OAuthContext {
    OAuthContext {
        adapter_id: "napcat".to_owned(),
        group_id: "group:10001".to_owned(),
        bot_qq: "bot:20002".to_owned(),
    }
}

fn token(access: &str, refresh: &str) -> NewOAuthToken {
    NewOAuthToken::new(
        SecretString::from(access.to_owned()),
        SecretString::from(refresh.to_owned()),
        "Bearer".to_owned(),
        Some("read_player".to_owned()),
        "client-id".to_owned(),
        Some(2_000),
    )
}

async fn authorization(store: &StateStore, subject: &str) -> TestResult {
    let state = if subject == "subject-1" {
        "opaque-state-sentinel".to_owned()
    } else {
        format!("opaque-state-for-{subject}")
    };
    store
        .save_oauth_authorization(&NewOAuthAuthorization::new(
            subject.to_owned(),
            SecretString::from(state),
            SecretString::from(
                "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~".to_owned(),
            ),
            Some(context()),
            1_000,
            1_600,
        ))
        .await?;
    Ok(())
}

#[tokio::test]
async fn secrets_and_tokens_survive_reopen_without_debug_disclosure() -> TestResult {
    let temp = TempDir::new()?;
    let path = temp.path().join("oauth.db");
    let store = StateStore::open(&path).await?;
    authorization(&store, "subject-1").await?;
    let claim = store
        .claim_oauth_authorization(
            "subject-1",
            Some(&SecretString::from("opaque-state-sentinel".to_owned())),
            None,
            Some(&context()),
            1_100,
        )
        .await?;
    let AuthorizationClaimResult::Claimed(claim) = claim else {
        return Err("authorization was not claimed".into());
    };
    let record = store
        .commit_oauth_authorization(
            "subject-1",
            claim.authorization.generation,
            &token("access-secret-sentinel", "refresh-secret-sentinel"),
            1_100,
        )
        .await?
        .ok_or("authorization commit lost")?;
    assert_eq!(record.subject, "subject-1");
    store.close().await;

    let reopened = StateStore::open(&path).await?;
    let record = reopened
        .oauth_token("subject-1")
        .await?
        .ok_or("token missing after reopen")?;
    assert_eq!(
        record.access_token().expose_secret(),
        "access-secret-sentinel"
    );
    assert_eq!(
        record.refresh_token().expose_secret(),
        "refresh-secret-sentinel"
    );
    let debug = format!("{record:?}");
    assert!(!debug.contains("access-secret-sentinel"));
    assert!(!debug.contains("refresh-secret-sentinel"));
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn authorization_claim_and_poke_confirmation_are_atomic() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    authorization(&store, "subject-1").await?;
    let barrier = Arc::new(Barrier::new(3));
    let mut claims = Vec::new();
    for _ in 0..2 {
        let store = store.clone();
        let barrier = barrier.clone();
        claims.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .claim_oauth_authorization("subject-1", None, None, Some(&context()), 1_100)
                .await
        }));
    }
    barrier.wait().await;
    let first = claims.remove(0).await??;
    let second = claims.remove(0).await??;
    let generation = match (first, second) {
        (AuthorizationClaimResult::Claimed(claim), AuthorizationClaimResult::Busy)
        | (AuthorizationClaimResult::Busy, AuthorizationClaimResult::Claimed(claim)) => {
            claim.authorization.generation
        }
        _ => return Err("concurrent claims did not produce one winner".into()),
    };

    store
        .save_oauth_pending_poke(&OAuthPendingPoke {
            subject: "subject-1".to_owned(),
            authorization_generation: generation,
            context: context(),
            token: token("pending-access", "pending-refresh"),
            created_at: 1_100,
            expires_at: 1_400,
        })
        .await?;
    let barrier = Arc::new(Barrier::new(3));
    let mut confirmations = Vec::new();
    for _ in 0..2 {
        let store = store.clone();
        let barrier = barrier.clone();
        confirmations.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .confirm_oauth_pending_poke("subject-1", &context(), 1_200)
                .await
        }));
    }
    barrier.wait().await;
    let first = confirmations.remove(0).await??;
    let second = confirmations.remove(0).await??;
    assert!(matches!(
        (&first, &second),
        (
            OAuthConfirmResult::Confirmed(_),
            OAuthConfirmResult::NotFound
        ) | (
            OAuthConfirmResult::NotFound,
            OAuthConfirmResult::Confirmed(_)
        )
    ));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn context_mismatch_precedes_expiry_and_expired_pending_is_removed() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("oauth.db")).await?;
    authorization(&store, "subject-1").await?;
    let claim = store
        .claim_oauth_authorization("subject-1", None, None, Some(&context()), 1_050)
        .await?;
    let AuthorizationClaimResult::Claimed(claim) = claim else {
        return Err("authorization was not claimed".into());
    };
    store
        .save_oauth_pending_poke(&OAuthPendingPoke {
            subject: "subject-1".to_owned(),
            authorization_generation: claim.authorization.generation,
            context: context(),
            token: token("pending-access", "pending-refresh"),
            created_at: 1_000,
            expires_at: 1_100,
        })
        .await?;
    let mut wrong = context();
    wrong.group_id = "group:wrong".to_owned();
    assert!(matches!(
        store
            .confirm_oauth_pending_poke("subject-1", &wrong, 1_200)
            .await?,
        OAuthConfirmResult::ContextMismatch
    ));
    assert!(matches!(
        store
            .confirm_oauth_pending_poke("subject-1", &context(), 1_200)
            .await?,
        OAuthConfirmResult::Expired
    ));
    assert!(
        store
            .oauth_pending_poke("subject-1", 1_200)
            .await?
            .is_none()
    );
    assert!(
        store
            .oauth_authorization("subject-1", 1_200)
            .await?
            .is_none()
    );
    authorization(&store, "subject-2").await?;
    assert!(matches!(
        store
            .claim_oauth_authorization("subject-2", None, None, Some(&context()), 1_600,)
            .await?,
        AuthorizationClaimResult::Expired
    ));
    assert!(
        store
            .oauth_authorization("subject-2", 1_600)
            .await?
            .is_none()
    );
    authorization(&store, "subject-3").await?;
    let claim = store
        .claim_oauth_authorization("subject-3", None, None, Some(&context()), 1_050)
        .await?;
    let AuthorizationClaimResult::Claimed(claim) = claim else {
        return Err("authorization was not claimed".into());
    };
    store
        .save_oauth_pending_poke(&OAuthPendingPoke {
            subject: "subject-3".to_owned(),
            authorization_generation: claim.authorization.generation,
            context: context(),
            token: token("pending-access-3", "pending-refresh-3"),
            created_at: 1_050,
            expires_at: 1_100,
        })
        .await?;
    assert!(
        store
            .oauth_pending_poke("subject-3", 1_200)
            .await?
            .is_none()
    );
    assert!(
        store
            .oauth_authorization("subject-3", 1_200)
            .await?
            .is_none()
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn legacy_binding_oauth_is_migrated_once_without_mutating_source_json() -> TestResult {
    let temp = TempDir::new()?;
    let path = temp.path().join("legacy-oauth.db");
    let database_url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;
    sqlx::query(
        "CREATE TABLE user_bindings (qq TEXT PRIMARY KEY, record_json TEXT NOT NULL, updated_at TEXT NOT NULL)",
    )
    .execute(&pool)
    .await?;
    let legacy = json!({
        "lxnsOAuth": {
            "accessToken": "legacy-access-secret-sentinel",
            "refreshToken": "legacy-refresh-secret-sentinel",
            "tokenType": "Bearer",
            "scope": "read_player write_player",
            "clientId": "legacy-client-id",
            "expiresAt": "2030-01-01T00:00:00Z",
            "boundAt": "2026-01-02T03:04:05Z",
            "updatedAt": "2026-01-03T04:05:06Z"
        },
        "unrelated": {"preserved": true}
    });
    let legacy_text = serde_json::to_string(&legacy)?;
    sqlx::query("INSERT INTO user_bindings (qq, record_json, updated_at) VALUES (?, ?, ?)")
        .bind("1000000001")
        .bind(&legacy_text)
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO user_bindings (qq, record_json, updated_at) VALUES (?, ?, ?)")
        .bind("1000000002")
        .bind(r#"{"lxnsOAuth":{"accessToken":"bad\ntoken","clientId":"client"}}"#)
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO user_bindings (qq, record_json, updated_at) VALUES (?, ?, ?)")
        .bind("1000000003")
        .bind(r#"{"lxnsOAuth":{"accessToken":"legacy-access-only"}}"#)
        .bind("2026-01-04T05:06:07Z")
        .execute(&pool)
        .await?;
    pool.close().await;

    let store = StateStore::open(&path).await?;
    let token = store
        .oauth_token("1000000001")
        .await?
        .ok_or("legacy OAuth token was not migrated")?;
    assert_eq!(
        token.access_token().expose_secret(),
        "legacy-access-secret-sentinel"
    );
    assert_eq!(
        token.refresh_token().expose_secret(),
        "legacy-refresh-secret-sentinel"
    );
    assert_eq!(token.token_type, "Bearer");
    assert_eq!(token.scope.as_deref(), Some("read_player write_player"));
    assert_eq!(token.client_id, "legacy-client-id");
    assert_eq!(
        token.expires_at,
        Some(OffsetDateTime::parse("2030-01-01T00:00:00Z", &Rfc3339)?.unix_timestamp())
    );
    assert_eq!(
        token.bound_at,
        OffsetDateTime::parse("2026-01-02T03:04:05Z", &Rfc3339)?.unix_timestamp()
    );
    assert_eq!(
        token.updated_at,
        OffsetDateTime::parse("2026-01-03T04:05:06Z", &Rfc3339)?.unix_timestamp()
    );
    let source: String =
        sqlx::query_scalar("SELECT record_json FROM user_bindings WHERE qq = '1000000001'")
            .fetch_one(&store.pool)
            .await?;
    assert_eq!(source, legacy_text);
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT imported_count, skipped_count FROM maimai_storage_migrations WHERE name = 'lxns_oauth_legacy_binding_v1'",
    )
    .fetch_one(&store.pool)
    .await?;
    assert_eq!(counts, (2, 1));
    let access_only = store
        .oauth_token("1000000003")
        .await?
        .ok_or("access-only legacy OAuth token was not migrated")?;
    assert_eq!(
        access_only.access_token().expose_secret(),
        "legacy-access-only"
    );
    assert_eq!(access_only.refresh_token().expose_secret(), "");
    assert_eq!(
        access_only.client_id,
        "f046fb83-f5c5-436d-bdfb-45c555638aa4"
    );
    assert_eq!(
        access_only.scope.as_deref(),
        Some("write_player read_user_profile read_player")
    );

    sqlx::query("UPDATE user_bindings SET record_json = ? WHERE qq = '1000000001'")
        .bind(r#"{"lxnsOAuth":{"accessToken":"replacement","refreshToken":"replacement","clientId":"replacement"}}"#)
        .execute(&store.pool)
        .await?;
    store.close().await;

    let reopened = StateStore::open(&path).await?;
    let token = reopened
        .oauth_token("1000000001")
        .await?
        .ok_or("migrated OAuth token disappeared after reopen")?;
    assert_eq!(
        token.access_token().expose_secret(),
        "legacy-access-secret-sentinel"
    );
    reopened.close().await;
    Ok(())
}
