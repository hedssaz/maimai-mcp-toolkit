use std::{error::Error, fs, future, sync::Arc, time::Duration};

use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_providers::{CatalogSource, CatalogSourceClient, CatalogSourceConfig};
use maimai_storage::{CatalogRefreshJobStart, CatalogRefreshJobStatus, CatalogRefreshJobStore};
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::sync::Notify;

use super::{CatalogRefreshJobs, RefreshJobError};
use crate::catalog_refresh::{EnabledSources, RefreshRequest};

#[tokio::test]
async fn second_coordinator_reports_owner_without_interrupting_first() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new().await?;
    let jobs = fixture.jobs().await?;
    let started = jobs
        .start_with(fixture.request()?, |_, _| {
            future::pending::<Result<_, crate::catalog_refresh::RefreshError>>()
        })
        .await?;
    wait_running(&jobs, started.record().id).await?;
    let second_store = CatalogRefreshJobStore::open(&fixture.database).await?;
    let error =
        match CatalogRefreshJobs::open(Arc::clone(&fixture.service), second_store.clone()).await {
            Ok(_) => return Err("second coordinator acquired an active state database".into()),
            Err(error) => error,
        };
    assert!(matches!(
        error,
        RefreshJobError::AlreadyRunning {
            lock_path: _,
            active_job_id: Some(id),
        } if id == started.record().id
    ));
    assert_eq!(
        jobs.status(started.record().id)
            .await?
            .ok_or("job missing")?
            .status,
        CatalogRefreshJobStatus::Running
    );
    jobs.shutdown().await?;
    let replacement = Arc::new(
        CatalogRefreshJobs::open(Arc::clone(&fixture.service), second_store.clone()).await?,
    );
    assert_eq!(
        replacement
            .status(started.record().id)
            .await?
            .ok_or("job missing after shutdown")?
            .status,
        CatalogRefreshJobStatus::Interrupted
    );
    replacement.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn new_owner_interrupts_job_abandoned_without_a_live_lock() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let started = fixture
        .state
        .start_catalog_refresh_job(
            &["plate".to_owned()],
            &["plate".to_owned()],
            OffsetDateTime::now_utc(),
        )
        .await?;
    let job = match started {
        CatalogRefreshJobStart::Started(job) => job,
        CatalogRefreshJobStart::AlreadyRunning(_) => return Err("unexpected active job".into()),
    };
    let jobs = fixture.jobs().await?;
    assert_eq!(
        jobs.status(job.id).await?.ok_or("job missing")?.status,
        CatalogRefreshJobStatus::Interrupted
    );
    jobs.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn background_job_persists_progress_and_terminal_outcome() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let jobs = fixture.jobs().await?;
    let started = jobs.start(fixture.request()?).await?;
    assert!(started.started());
    let terminal = wait_terminal(&jobs, started.record().id).await?;
    assert_eq!(terminal.status, CatalogRefreshJobStatus::Completed);
    assert_eq!(terminal.completed_sources(), 1);
    assert_eq!(terminal.succeeded_sources(), vec!["plate"]);
    assert!(terminal.sources[0].status.completed());
    jobs.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn start_returns_real_expired_plan_for_missing_target() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    fs::remove_file(&fixture.plate)?;
    let jobs = fixture.jobs().await?;
    let started = jobs
        .start_with(fixture.request()?, |_, _| {
            future::pending::<Result<_, crate::catalog_refresh::RefreshError>>()
        })
        .await?;
    assert!(started.record().sources[0].due);
    let plan = started.plan().ok_or("refresh plan missing")?;
    assert_eq!(plan.due_sources(), &[CatalogSource::Plate]);
    assert!(plan.statuses()[0].expired());
    assert!(plan.statuses()[0].age().is_none());
    jobs.runtime
        .lock()
        .map_err(|_| "runtime poisoned")?
        .active
        .as_ref()
        .ok_or("active task missing")?
        .task
        .abort();
    let _ = wait_terminal(&jobs, started.record().id).await?;
    Ok(())
}

#[tokio::test]
async fn concurrent_starts_share_one_database_flight() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let jobs = fixture.jobs().await?;
    let result = Arc::new(
        fixture
            .service
            .refresh(fixture.request()?.check_only_copy())
            .await?,
    );
    let release = Arc::new(Notify::new());
    let first = jobs
        .start_with(fixture.request()?, {
            let release = Arc::clone(&release);
            let result = Arc::clone(&result);
            move |_, _| async move {
                release.notified().await;
                Ok((*result).clone())
            }
        })
        .await?;
    let second = jobs
        .start_with(fixture.request()?, |_, _| async {
            Err(crate::catalog_refresh::RefreshError::InvalidTimeout)
        })
        .await?;
    assert!(first.started());
    assert!(!second.started());
    assert_eq!(first.record().id, second.record().id);
    // `notify_one` retains a permit if the spawned job has not reached
    // `notified()` yet; `notify_waiters` would lose that wakeup.
    release.notify_one();
    assert!(
        wait_terminal(&jobs, first.record().id)
            .await?
            .status
            .is_terminal()
    );
    jobs.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn aborted_and_panicked_tasks_are_reaped_as_failed() -> Result<(), Box<dyn Error>> {
    for panic_task in [false, true] {
        let fixture = Fixture::new().await?;
        let jobs = fixture.jobs().await?;
        let started = if panic_task {
            jobs.start_with(fixture.request()?, |_, _| async move {
                panic!("catalog refresh fixture panic")
            })
            .await?
        } else {
            jobs.start_with(fixture.request()?, |_, _| {
                future::pending::<Result<_, crate::catalog_refresh::RefreshError>>()
            })
            .await?
        };
        if !panic_task {
            wait_running(&jobs, started.record().id).await?;
            jobs.runtime
                .lock()
                .map_err(|_| "runtime poisoned")?
                .active
                .as_ref()
                .ok_or("active task missing")?
                .task
                .abort();
        }
        let terminal = wait_terminal(&jobs, started.record().id).await?;
        assert_eq!(terminal.status, CatalogRefreshJobStatus::Failed);
        assert_eq!(terminal.error_code.as_deref(), Some("TASK_ABORTED"));
        jobs.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn failed_terminal_write_is_retried_by_status() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let jobs = fixture.jobs().await?;
    let result = Arc::new(
        fixture
            .service
            .refresh(fixture.request()?.check_only_copy())
            .await?,
    );
    let release = Arc::new(Notify::new());
    let started = jobs
        .start_with(fixture.request()?, {
            let release = Arc::clone(&release);
            move |_, _| async move {
                release.notified().await;
                Ok((*result).clone())
            }
        })
        .await?;
    wait_running(&jobs, started.record().id).await?;
    let pool =
        sqlx::SqlitePool::connect(&format!("sqlite://{}", fixture.database.display())).await?;
    sqlx::query(
        "CREATE TRIGGER fail_refresh_terminal BEFORE UPDATE OF status ON catalog_refresh_jobs WHEN NEW.status IN ('completed','failed') BEGIN SELECT RAISE(FAIL, 'fixture failure'); END",
    )
    .execute(&pool)
    .await?;
    release.notify_waiters();
    wait_task_finished(&jobs).await?;
    assert!(jobs.status(started.record().id).await.is_err());
    sqlx::query("DROP TRIGGER fail_refresh_terminal")
        .execute(&pool)
        .await?;
    assert_eq!(
        wait_terminal(&jobs, started.record().id).await?.status,
        CatalogRefreshJobStatus::Completed
    );
    pool.close().await;
    jobs.shutdown().await?;
    Ok(())
}

struct Fixture {
    _root: TempDir,
    database: std::path::PathBuf,
    state: CatalogRefreshJobStore,
    service: Arc<crate::catalog_refresh::CatalogRefreshService>,
    plate: std::path::PathBuf,
}

impl Fixture {
    async fn new() -> Result<Self, Box<dyn Error>> {
        let root = TempDir::new()?;
        let mut files = CatalogFiles::from_data_dir(fs::canonicalize(root.path())?);
        files.official_music_data = None;
        files.legacy_aliases_csv = None;
        files.artist_aliases = None;
        files.charter_aliases = None;
        files.traditional_to_simplified = None;
        write_catalog(&files)?;
        let plate = files.maimaidxplate.clone().ok_or("plate path missing")?;
        fs::write(&plate, "{}")?;
        let catalog = Arc::new(CatalogStore::load(files).await?);
        let service = Arc::new(crate::catalog_refresh::CatalogRefreshService::new(
            Arc::new(CatalogSourceClient::new(CatalogSourceConfig::default())?),
            catalog,
            EnabledSources::public(),
        )?);
        let database = root.path().join("state.db");
        let state = CatalogRefreshJobStore::open(&database).await?;
        Ok(Self {
            _root: root,
            database,
            state,
            service,
            plate,
        })
    }

    async fn jobs(&self) -> Result<Arc<CatalogRefreshJobs>, super::RefreshJobError> {
        Ok(Arc::new(
            CatalogRefreshJobs::open(Arc::clone(&self.service), self.state.clone()).await?,
        ))
    }

    fn request(&self) -> Result<RefreshRequest, crate::catalog_refresh::RefreshError> {
        RefreshRequest::new(
            vec![CatalogSource::Plate],
            365.0,
            false,
            false,
            Duration::from_secs(1),
        )
    }
}

async fn wait_terminal(
    jobs: &CatalogRefreshJobs,
    id: maimai_storage::CatalogRefreshJobId,
) -> Result<maimai_storage::CatalogRefreshJob, Box<dyn Error>> {
    for _ in 0..100 {
        if let Some(job) = jobs.status(id).await?
            && job.status.is_terminal()
        {
            return Ok(job);
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Err("job did not become terminal".into())
}

async fn wait_running(
    jobs: &CatalogRefreshJobs,
    id: maimai_storage::CatalogRefreshJobId,
) -> Result<(), Box<dyn Error>> {
    for _ in 0..100 {
        if jobs
            .status(id)
            .await?
            .is_some_and(|job| job.status == CatalogRefreshJobStatus::Running)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Err("job did not start running".into())
}

async fn wait_task_finished(jobs: &CatalogRefreshJobs) -> Result<(), Box<dyn Error>> {
    for _ in 0..100 {
        if jobs
            .runtime
            .lock()
            .map_err(|_| "runtime poisoned")?
            .active
            .as_ref()
            .is_some_and(|active| active.task.is_finished())
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Err("job task did not finish".into())
}

fn write_catalog(files: &CatalogFiles) -> Result<(), Box<dyn Error>> {
    fs::write(
        &files.lxns_song_list,
        r#"{"songs":[],"genres":[],"versions":[]}"#,
    )?;
    fs::write(&files.diving_fish_song_list, "[]")?;
    fs::write(&files.lxns_alias_list, r#"{"aliases":[]}"#)?;
    fs::write(&files.yuzu_alias_list, r#"{"content":[]}"#)?;
    fs::write(&files.custom_aliases, "{}")?;
    fs::write(&files.pinyin_aliases, r#"{"aliases":[]}"#)?;
    fs::write(&files.simplified_to_traditional, "{}")?;
    fs::write(
        files.dxdata.as_ref().ok_or("dxdata path")?,
        r#"{"songs":[],"versions":[]}"#,
    )?;
    fs::write(
        files.chart_stats.as_ref().ok_or("stats path")?,
        r#"{"charts":{},"diff_data":{}}"#,
    )?;
    fs::write(
        files.tags.as_ref().ok_or("tags path")?,
        r#"{"tags":[],"tagGroups":[],"tagSongs":[]}"#,
    )?;
    fs::write(files.dxrating_aliases.as_ref().ok_or("alias path")?, "[]")?;
    Ok(())
}
