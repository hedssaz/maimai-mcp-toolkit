use sqlx::SqlitePool;

use super::migration::migrate_legacy_oauth_tokens;
use crate::StorageError;

const TABLES: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS lxns_oauth_authorizations (
        subject TEXT PRIMARY KEY,
        generation INTEGER NOT NULL,
        state TEXT NOT NULL,
        code_verifier TEXT NOT NULL,
        adapter_id TEXT,
        group_id TEXT,
        bot_qq TEXT,
        status TEXT NOT NULL CHECK (status IN ('pending', 'exchanging')),
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    )
    "#,
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_lxns_oauth_authorization_state ON lxns_oauth_authorizations(state)",
    r#"
    CREATE TABLE IF NOT EXISTS lxns_oauth_tokens (
        subject TEXT PRIMARY KEY,
        generation INTEGER NOT NULL,
        access_token TEXT NOT NULL,
        refresh_token TEXT NOT NULL,
        token_type TEXT NOT NULL,
        scope TEXT,
        client_id TEXT NOT NULL,
        expires_at INTEGER,
        bound_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS lxns_oauth_pending_pokes (
        subject TEXT PRIMARY KEY,
        authorization_generation INTEGER NOT NULL,
        adapter_id TEXT NOT NULL,
        group_id TEXT NOT NULL,
        bot_qq TEXT NOT NULL,
        access_token TEXT NOT NULL,
        refresh_token TEXT NOT NULL,
        token_type TEXT NOT NULL,
        scope TEXT,
        client_id TEXT NOT NULL,
        token_expires_at INTEGER,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    )
    "#,
];

pub(crate) async fn initialize_oauth_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in TABLES {
        sqlx::query(*statement).execute(pool).await?;
    }
    migrate_legacy_oauth_tokens(pool).await
}
