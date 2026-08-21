use std::error::Error;

use secrecy::{ExposeSecret, SecretString};
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::StateStore;

#[tokio::test]
async fn token_reopens_overwrites_and_clears_idempotently()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    let path = temp.path().join("state.db");
    let first_time = OffsetDateTime::from_unix_timestamp(1_700_000_000)?;
    let second_time = OffsetDateTime::from_unix_timestamp(1_700_000_100)?;
    let store = StateStore::open(&path).await?;
    store
        .set_diving_fish_developer_token(
            &SecretString::from("first-secret-sentinel".to_owned()),
            first_time,
        )
        .await?;
    store
        .set_diving_fish_developer_token(
            &SecretString::from("second-secret-sentinel".to_owned()),
            second_time,
        )
        .await?;
    store.close().await;

    let reopened = StateStore::open(&path).await?;
    let token = reopened
        .diving_fish_developer_token()
        .await?
        .ok_or("token missing after reopen")?;
    assert_eq!(token.token().expose_secret(), "second-secret-sentinel");
    assert_eq!(token.updated_at, second_time);
    let debug = format!("{token:?}");
    assert!(!debug.contains("first-secret-sentinel"));
    assert!(!debug.contains("second-secret-sentinel"));
    assert!(reopened.clear_diving_fish_developer_token().await?);
    assert!(!reopened.clear_diving_fish_developer_token().await?);
    reopened.close().await;
    Ok(())
}
