use std::error::Error;

use tempfile::TempDir;
use time::OffsetDateTime;

use super::{
    CatalogRefreshJobOutcome, CatalogRefreshJobStart, CatalogRefreshJobStatus,
    CatalogRefreshJobStore, CatalogRefreshOwnerClaim, CatalogRefreshSourceStatus,
    CatalogRefreshSourceUpdate, CatalogRefreshTerminalUpdate,
};

#[tokio::test]
async fn job_crud_is_single_flight_and_uses_terminal_cas() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let store = CatalogRefreshJobStore::open(temp.path().join("state.db")).await?;
    let sources = vec!["lxns".to_owned(), "plate".to_owned()];
    let started = store
        .start_catalog_refresh_job(&sources, &["lxns".to_owned()], OffsetDateTime::now_utc())
        .await?;
    let CatalogRefreshJobStart::Started(job) = started else {
        return Err("first job must start".into());
    };
    assert_eq!(job.completed_sources(), 1);
    assert_eq!(job.sources[0].status, CatalogRefreshSourceStatus::Queued);
    assert_eq!(job.sources[1].status, CatalogRefreshSourceStatus::Skipped);
    let immediately_visible = store
        .catalog_refresh_job(job.id)
        .await?
        .ok_or("queued job missing")?;
    assert_eq!(
        immediately_visible.sources[0].status,
        CatalogRefreshSourceStatus::Queued
    );
    assert_eq!(
        immediately_visible.sources[1].status,
        CatalogRefreshSourceStatus::Skipped
    );
    assert_eq!(immediately_visible.completed_sources(), 1);
    let duplicate = store
        .start_catalog_refresh_job(&sources, &["lxns".to_owned()], OffsetDateTime::now_utc())
        .await?;
    assert!(matches!(
        duplicate,
        CatalogRefreshJobStart::AlreadyRunning(_)
    ));
    assert!(
        store
            .mark_catalog_refresh_job_running(job.id, OffsetDateTime::now_utc())
            .await?
    );
    assert!(
        store
            .plan_catalog_refresh_job(job.id, &["lxns".to_owned()], &["plate".to_owned()])
            .await?
    );
    assert!(
        store
            .update_catalog_refresh_source(
                job.id,
                &CatalogRefreshSourceUpdate {
                    source: "lxns".to_owned(),
                    status: CatalogRefreshSourceStatus::Updated,
                    duration_millis: 12,
                    error_code: None,
                    error_message: None,
                },
            )
            .await?
    );
    assert!(
        store
            .finish_catalog_refresh_job(
                job.id,
                OffsetDateTime::now_utc(),
                &CatalogRefreshTerminalUpdate {
                    status: CatalogRefreshJobStatus::Completed,
                    outcome: CatalogRefreshJobOutcome::Success,
                    message: "done".to_owned(),
                    error_code: None,
                    error_message: None,
                },
            )
            .await?
    );
    assert!(
        !store
            .finish_catalog_refresh_job(
                job.id,
                OffsetDateTime::now_utc(),
                &CatalogRefreshTerminalUpdate {
                    status: CatalogRefreshJobStatus::Failed,
                    outcome: CatalogRefreshJobOutcome::Failed,
                    message: "late".to_owned(),
                    error_code: Some("LATE".to_owned()),
                    error_message: Some("late".to_owned()),
                },
            )
            .await?
    );
    let stored = store
        .catalog_refresh_job(job.id)
        .await?
        .ok_or("job missing")?;
    assert_eq!(stored.status, CatalogRefreshJobStatus::Completed);
    assert_eq!(stored.completed_sources(), 2);
    assert_eq!(stored.succeeded_sources(), vec!["lxns", "plate"]);
    assert_eq!(stored.skipped_sources(), vec!["plate"]);
    Ok(())
}

#[tokio::test]
async fn targeted_interrupt_clears_only_the_selected_job() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let store = CatalogRefreshJobStore::open(temp.path().join("state.db")).await?;
    let started = store
        .start_catalog_refresh_job(
            &["lxns".to_owned()],
            &["lxns".to_owned()],
            OffsetDateTime::now_utc(),
        )
        .await?;
    let job = match started {
        CatalogRefreshJobStart::Started(job) => job,
        CatalogRefreshJobStart::AlreadyRunning(_) => return Err("unexpected active job".into()),
    };
    assert!(
        store
            .interrupt_catalog_refresh_job(job.id, OffsetDateTime::now_utc())
            .await?
    );
    let stored = store
        .catalog_refresh_job(job.id)
        .await?
        .ok_or("job missing")?;
    assert_eq!(stored.status, CatalogRefreshJobStatus::Interrupted);
    assert_eq!(stored.outcome, Some(CatalogRefreshJobOutcome::Interrupted));
    assert_eq!(stored.failed_sources(), vec!["lxns"]);
    Ok(())
}

#[tokio::test]
async fn narrow_store_initializes_only_catalog_refresh_tables() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let database = temp.path().join("state.db");
    let store = CatalogRefreshJobStore::open(&database).await?;
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", database.display())).await?;
    let tables = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        tables,
        vec![
            "catalog_refresh_job_sources",
            "catalog_refresh_jobs",
            "sqlite_sequence",
        ]
    );
    pool.close().await;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn general_state_store_does_not_initialize_catalog_refresh_tables()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let database = temp.path().join("state.db");
    let state = crate::StateStore::open(&database).await?;
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", database.display())).await?;
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name LIKE 'catalog_refresh_%'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(count, 0);
    pool.close().await;
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn owner_claim_is_atomic_and_requires_explicit_release() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let database = temp.path().join("state.db");
    let first_store = CatalogRefreshJobStore::open(&database).await?;
    let second_store = CatalogRefreshJobStore::open(&database).await?;
    let first = match first_store.claim_owner()? {
        CatalogRefreshOwnerClaim::Acquired(owner) => owner,
        CatalogRefreshOwnerClaim::AlreadyOwned { .. } => {
            return Err("first owner was rejected".into());
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            std::fs::metadata(first.lock_path())?.permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(matches!(
        second_store.claim_owner()?,
        CatalogRefreshOwnerClaim::AlreadyOwned { .. }
    ));
    assert!(first_store.release_owner(&first)?);
    let replacement = match second_store.claim_owner()? {
        CatalogRefreshOwnerClaim::Acquired(owner) => owner,
        CatalogRefreshOwnerClaim::AlreadyOwned { .. } => {
            return Err("owner lock remained held after release".into());
        }
    };
    drop(replacement);
    assert!(matches!(
        first_store.claim_owner()?,
        CatalogRefreshOwnerClaim::Acquired(_)
    ));
    first_store.close().await;
    second_store.close().await;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn owner_lock_rejects_symlink_and_non_regular_paths() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new()?;
    let database = temp.path().join("state.db");
    let store = CatalogRefreshJobStore::open(&database).await?;
    let owner = match store.claim_owner()? {
        CatalogRefreshOwnerClaim::Acquired(owner) => owner,
        CatalogRefreshOwnerClaim::AlreadyOwned { .. } => {
            return Err("first owner was rejected".into());
        }
    };
    let lock_path = owner.lock_path().to_path_buf();
    assert!(store.release_owner(&owner)?);
    std::fs::remove_file(&lock_path)?;
    symlink(&database, &lock_path)?;
    assert!(matches!(
        store.claim_owner(),
        Err(crate::StorageError::PreparePath { .. })
    ));
    std::fs::remove_file(&lock_path)?;
    std::fs::create_dir(&lock_path)?;
    assert!(matches!(
        store.claim_owner(),
        Err(crate::StorageError::PreparePath { .. })
    ));
    store.close().await;
    Ok(())
}
