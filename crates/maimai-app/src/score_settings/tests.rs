use std::error::Error;

use maimai_core::{QqId, ScoreSource};
use tempfile::TempDir;
use time::OffsetDateTime;

use super::{AllowedScoreSources, DeveloperToken, ScoreSettingsErrorCode, ScoreSettingsService};
use maimai_storage::StateStore;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn bind_overwrites_status_is_safe_and_clear_is_idempotent() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let service = ScoreSettingsService::new(store.clone(), AllowedScoreSources::main());
    let first = OffsetDateTime::from_unix_timestamp(1_700_000_000)?;
    let second = OffsetDateTime::from_unix_timestamp(1_700_000_100)?;
    service
        .bind_developer_token(DeveloperToken::new("first-secret-sentinel")?, first)
        .await?;
    let rebound = service
        .bind_developer_token(DeveloperToken::new("second-secret-sentinel")?, second)
        .await?;
    assert!(rebound.bound);
    assert_eq!(rebound.updated_at, Some(second));
    let rendered = format!("{rebound:?} {:?}", service.developer_token_status().await?);
    assert!(!rendered.contains("first-secret-sentinel"));
    assert!(!rendered.contains("second-secret-sentinel"));
    assert!(service.clear_developer_token().await?.cleared);
    assert!(!service.clear_developer_token().await?.cleared);
    assert!(!service.developer_token_status().await?.bound);
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn composition_allowlists_main_lxns_but_public_rejects_it() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let qq = QqId::new("10001")?;
    let main = ScoreSettingsService::new(store.clone(), AllowedScoreSources::main());
    main.switch_score_source(qq.clone(), ScoreSource::Lxns)
        .await?;
    assert_eq!(
        store.score_source_preference(&qq).await?,
        Some(ScoreSource::Lxns)
    );

    let public = ScoreSettingsService::new(store.clone(), AllowedScoreSources::public());
    let error = public
        .switch_score_source(qq.clone(), ScoreSource::Lxns)
        .await
        .err()
        .ok_or("public accepted lxns")?;
    assert_eq!(error.code(), ScoreSettingsErrorCode::SourceNotAllowed);
    assert!(
        public
            .switch_score_source(qq.clone(), ScoreSource::DivingFish)
            .await
            .is_ok()
    );
    assert!(
        public
            .switch_score_source(qq.clone(), ScoreSource::Local)
            .await
            .is_ok()
    );
    assert!(
        main.switch_score_source(qq, ScoreSource::OfficialCn)
            .await
            .is_err()
    );
    store.close().await;
    Ok(())
}
