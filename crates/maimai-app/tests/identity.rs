use std::{error::Error, io, sync::Arc, time::Duration};

use maimai_app::identity::{
    GroupDelay, IdentityDirectory, IdentityField, IdentityQuery, IdentityService, MaxGroups,
    MaxResults, NoCache, RefreshJobRequest, RefreshOptions, RefreshPolicy, ResetHour,
};
use maimai_core::{GroupId, PlayerUsername, QqId};
use maimai_providers::{NapCatClient, NapCatConfig};
use maimai_storage::{
    IdentityGroupSnapshot, IdentityJob, IdentityJobStart, IdentityJobStatus, IdentityRefreshReason,
    IdentitySnapshot, IdentitySnapshotMember, StateStore, WaterfishIdentityProfile,
};
use serde_json::{Value, json};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use tempfile::TempDir;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
    time::{sleep, timeout},
};
use url::Url;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

struct MockResponse {
    status: u16,
    body: String,
    delay: Duration,
}

#[tokio::test]
async fn refresh_is_atomic_discards_remarks_and_preserves_waterfish_fields() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("identity.db");
    let store = StateStore::open(&database).await?;
    let qq = QqId::new("10001")?;
    store
        .upsert_waterfish_identity(
            &qq,
            &WaterfishIdentityProfile {
                nickname: Some("Waterfish Name".to_owned()),
                username: Some(PlayerUsername::new("waterfish-user")?),
                rating: Some(15_000),
            },
            timestamp(2026, Month::August, 18, 1, 0)?,
        )
        .await?;
    let responses = success_responses(Duration::ZERO);
    let (base_url, _) = mock_server(responses).await?;
    let override_client = client(base_url)?;
    let service = IdentityService::new(store.clone(), unused_client()?);

    let report = service
        .refresh_with_client(
            &override_client,
            RefreshOptions {
                no_cache: NoCache::new(true),
                group_delay: GroupDelay::new(Duration::ZERO)?,
                max_groups: None,
            },
            timestamp(2026, Month::August, 18, 2, 0)?,
        )
        .await?;
    assert!(report.performed);
    assert_eq!(report.metadata.generation, 1);

    let group_id = GroupId::new("20001")?;
    let identity = service
        .get_identity(&qq, Some(&group_id))
        .await?
        .ok_or_else(|| io::Error::other("identity missing"))?;
    assert_eq!(identity.qq_nickname.as_deref(), Some("Alice"));
    assert_eq!(identity.friend_nickname.as_deref(), Some("Alice"));
    assert_eq!(
        identity.waterfish_nickname.as_deref(),
        Some("Waterfish Name")
    );
    assert_eq!(
        identity
            .waterfish_username
            .as_ref()
            .map(PlayerUsername::as_str),
        Some("waterfish-user")
    );
    assert_eq!(identity.waterfish_rating, Some(15_000));
    assert_eq!(
        identity
            .preferred_group
            .as_ref()
            .map(|group| group.group_nickname.as_str()),
        Some("Captain")
    );

    let resolution = service
        .resolve_identity(
            &IdentityQuery::new("Captain")?,
            Some(&group_id),
            MaxResults::new(10)?,
        )
        .await?;
    assert_eq!(resolution.matches[0].score, 110);
    assert!(
        resolution.matches[0]
            .matched_fields
            .contains(&IdentityField::PreferredGroupCard)
    );

    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database)).await?;
    let schema: String = sqlx::query("SELECT sql FROM sqlite_master WHERE name = 'identity_users'")
        .fetch_one(&mut connection)
        .await?
        .try_get("sql")?;
    assert!(!schema.to_ascii_lowercase().contains("remark"));
    connection.close().await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn waterfish_only_identity_creates_status_without_fake_fetch_time() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let service = IdentityService::new(store.clone(), unused_client()?);
    let qq = QqId::new("10009")?;
    service
        .upsert_waterfish_identity(
            &qq,
            &WaterfishIdentityProfile {
                nickname: Some("Only Waterfish".to_owned()),
                username: Some(PlayerUsername::new("only-waterfish")?),
                rating: Some(12_345),
            },
            timestamp(2026, Month::August, 18, 1, 0)?,
        )
        .await?;

    let status = service
        .cache_status(
            timestamp(2026, Month::August, 18, 2, 0)?,
            ResetHour::new(14)?,
        )
        .await?;
    assert!(status.exists);
    assert!(!status.fresh);
    assert_eq!(status.fetched_at, None);
    assert_eq!(status.age_seconds, None);
    assert_eq!(status.generation, Some(0));
    assert_eq!(status.stats.unique_users, 1);
    let identity = service
        .get_identity(&qq, None)
        .await?
        .ok_or_else(|| io::Error::other("waterfish identity missing"))?;
    assert_eq!(identity.waterfish_rating, Some(12_345));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn failed_remote_refresh_keeps_the_previous_snapshot() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let old_time = timestamp(2026, Month::August, 17, 14, 0)?;
    store
        .replace_identity_snapshot(&snapshot(old_time, "Old Card", "10001", "20001")?)
        .await?;
    let previous = store
        .identity_metadata()
        .await?
        .map(|value| value.generation);
    let responses = vec![
        ok(json!([{"user_id": 10002, "nickname": "New"}])),
        ok(json!([{"group_id": 20002, "group_name": "New Group", "member_count": 1}])),
        MockResponse {
            status: 500,
            body: json!({"error": "failure"}).to_string(),
            delay: Duration::ZERO,
        },
    ];
    let (base_url, _) = mock_server(responses).await?;
    let service = IdentityService::new(store.clone(), client(base_url)?);

    assert!(
        service
            .refresh(
                RefreshOptions {
                    group_delay: GroupDelay::new(Duration::ZERO)?,
                    ..RefreshOptions::default()
                },
                timestamp(2026, Month::August, 18, 2, 0)?,
            )
            .await
            .is_err()
    );
    let identity = store
        .identity(&QqId::new("10001")?, Some(&GroupId::new("20001")?))
        .await?
        .ok_or_else(|| io::Error::other("old identity missing"))?;
    assert_eq!(
        identity
            .preferred_group
            .as_ref()
            .map(|group| group.group_nickname.as_str()),
        Some("Old Card")
    );
    assert_eq!(
        store
            .identity_metadata()
            .await?
            .map(|value| value.generation),
        previous
    );
    store.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_refreshes_share_one_remote_flight() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let (base_url, mut captured) =
        mock_server(success_responses(Duration::from_millis(50))).await?;
    let service = Arc::new(IdentityService::new(store.clone(), client(base_url)?));
    let fetched_at = timestamp(2026, Month::August, 18, 2, 0)?;
    let options = RefreshOptions {
        group_delay: GroupDelay::new(Duration::ZERO)?,
        ..RefreshOptions::default()
    };
    let first_service = Arc::clone(&service);
    let second_service = Arc::clone(&service);
    let first = tokio::spawn(async move { first_service.refresh(options, fetched_at).await });
    let second = tokio::spawn(async move { second_service.refresh(options, fetched_at).await });
    let first = first.await??;
    let second = second.await??;

    assert_ne!(first.performed, second.performed);
    assert_eq!(first.metadata.generation, second.metadata.generation);
    let mut requests = Vec::new();
    while let Some(request) = captured.recv().await {
        requests.push(request);
    }
    assert_eq!(requests.len(), 3);
    store.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_leader_does_not_strand_refresh_followers() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let (base_url, mut captured) =
        mock_server(success_responses(Duration::from_millis(100))).await?;
    let service = Arc::new(IdentityService::new(store.clone(), client(base_url)?));
    let fetched_at = timestamp(2026, Month::August, 18, 2, 0)?;
    let options = RefreshOptions {
        group_delay: GroupDelay::new(Duration::ZERO)?,
        ..RefreshOptions::default()
    };

    let leader_service = Arc::clone(&service);
    let leader = tokio::spawn(async move { leader_service.refresh(options, fetched_at).await });
    timeout(Duration::from_secs(1), captured.recv())
        .await?
        .ok_or_else(|| io::Error::other("first refresh request missing"))?;
    leader.abort();
    let cancellation = leader
        .await
        .err()
        .ok_or_else(|| io::Error::other("aborted leader completed normally"))?;
    assert!(cancellation.is_cancelled());

    let follower = timeout(Duration::from_secs(3), service.refresh(options, fetched_at)).await??;
    assert!(!follower.performed);
    assert_eq!(follower.metadata.generation, 1);
    assert_eq!(drain_requests(&mut captured).await.len(), 2);

    service.initialize_identity_jobs(fetched_at).await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn local_directory_resolves_targets_without_napcat_configuration() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let group_id = GroupId::new("20001")?;
    store
        .replace_identity_snapshot(&IdentitySnapshot {
            fetched_at: timestamp(2026, Month::August, 18, 2, 0)?,
            friends: Vec::new(),
            groups: vec![IdentityGroupSnapshot {
                group_id: group_id.clone(),
                group_name: Some("Mai Group".to_owned()),
                member_count: Some(1),
                members: vec![member("10001", "Alice", "Captain")?],
            }],
        })
        .await?;
    let directory = IdentityDirectory::new(store.clone());

    let resolution = directory
        .resolve_identity(
            &IdentityQuery::new("Captain")?,
            Some(&group_id),
            MaxResults::default_value(),
        )
        .await?;
    assert!(!resolution.ambiguous);
    assert_eq!(
        resolution
            .matches
            .first()
            .map(|candidate| candidate.identity.qq.as_str()),
        Some("10001")
    );
    assert_eq!(directory.cached_group_members(&group_id).await?.len(), 1);
    assert!(
        directory
            .cache_status(
                timestamp(2026, Month::August, 18, 2, 1)?,
                ResetHour::new(14)?,
            )
            .await?
            .exists
    );
    let qq = QqId::new("10001")?;
    directory
        .upsert_waterfish_identity(
            &qq,
            &WaterfishIdentityProfile {
                nickname: Some("Waterfish Alice".to_owned()),
                username: Some(PlayerUsername::new("waterfish-alice")?),
                rating: Some(14_000),
            },
            timestamp(2026, Month::August, 18, 2, 2)?,
        )
        .await?;
    assert_eq!(
        directory
            .get_identity(&qq, Some(&group_id))
            .await?
            .and_then(|identity| identity.waterfish_rating),
        Some(14_000)
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn resolution_keeps_duplicate_name_ambiguity_and_qq_sorting() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let fetched_at = timestamp(2026, Month::August, 18, 2, 0)?;
    let group_id = GroupId::new("20001")?;
    store
        .replace_identity_snapshot(&IdentitySnapshot {
            fetched_at,
            friends: Vec::new(),
            groups: vec![IdentityGroupSnapshot {
                group_id: group_id.clone(),
                group_name: Some("Mai Group".to_owned()),
                member_count: Some(2),
                members: vec![
                    member("10002", "Other", "Same")?,
                    member("10001", "Dup", "Same")?,
                ],
            }],
        })
        .await?;
    let service = IdentityService::new(store.clone(), unused_client()?);

    let result = service
        .resolve_identity(
            &IdentityQuery::new("same")?,
            Some(&group_id),
            MaxResults::new(10)?,
        )
        .await?;
    assert!(result.ambiguous);
    assert_eq!(
        result
            .matches
            .iter()
            .map(|candidate| candidate.identity.qq.as_str())
            .collect::<Vec<_>>(),
        ["10001", "10002"]
    );
    assert!(
        result
            .matches
            .iter()
            .all(|candidate| candidate.score == 110)
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn targeted_identity_query_keeps_the_requested_preferred_group() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let first_group = GroupId::new("20001")?;
    let preferred_group = GroupId::new("20002")?;
    store
        .replace_identity_snapshot(&IdentitySnapshot {
            fetched_at: timestamp(2026, Month::August, 18, 2, 0)?,
            friends: Vec::new(),
            groups: vec![
                IdentityGroupSnapshot {
                    group_id: first_group,
                    group_name: Some("Alpha".to_owned()),
                    member_count: Some(1),
                    members: vec![member("10001", "First", "First Card")?],
                },
                IdentityGroupSnapshot {
                    group_id: preferred_group.clone(),
                    group_name: Some("Beta".to_owned()),
                    member_count: Some(1),
                    members: vec![member("10001", "Second", "Preferred Card")?],
                },
            ],
        })
        .await?;
    let service = IdentityService::new(store.clone(), unused_client()?);

    let identity = service
        .get_identity(&QqId::new("10001")?, Some(&preferred_group))
        .await?
        .ok_or_else(|| io::Error::other("targeted identity missing"))?;

    assert_eq!(identity.groups.len(), 2);
    assert_eq!(
        identity
            .preferred_group
            .as_ref()
            .map(|group| group.group_nickname.as_str()),
        Some("Preferred Card")
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn daily_reset_freshness_respects_injected_boundary() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let service = IdentityService::new(store.clone(), unused_client()?);
    let reset_hour = ResetHour::new(14)?;

    store
        .replace_identity_snapshot(&IdentitySnapshot {
            fetched_at: timestamp(2026, Month::August, 18, 13, 59)?,
            friends: Vec::new(),
            groups: Vec::new(),
        })
        .await?;
    let at_reset = service
        .cache_status(timestamp(2026, Month::August, 18, 14, 0)?, reset_hour)
        .await?;
    assert!(!at_reset.fresh);

    store
        .replace_identity_snapshot(&IdentitySnapshot {
            fetched_at: timestamp(2026, Month::August, 18, 14, 0)?,
            friends: Vec::new(),
            groups: Vec::new(),
        })
        .await?;
    let exact = service
        .cache_status(timestamp(2026, Month::August, 18, 14, 0)?, reset_hour)
        .await?;
    assert!(exact.fresh);

    store
        .replace_identity_snapshot(&IdentitySnapshot {
            fetched_at: timestamp(2026, Month::August, 17, 14, 0)?,
            friends: Vec::new(),
            groups: Vec::new(),
        })
        .await?;
    let before_reset = service
        .cache_status(timestamp(2026, Month::August, 18, 13, 59)?, reset_hour)
        .await?;
    assert!(before_reset.fresh);
    store.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn background_refresh_returns_immediately_coalesces_and_honors_force() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let (default_url, mut default_requests) =
        mock_server(success_responses(Duration::from_millis(100))).await?;
    let (override_url, mut override_requests) =
        mock_server(success_responses(Duration::ZERO)).await?;
    let service = IdentityService::new(store.clone(), client(default_url)?);
    let now = OffsetDateTime::now_utc();
    let request = RefreshJobRequest {
        policy: RefreshPolicy::IfStale,
        options: RefreshOptions {
            group_delay: GroupDelay::new(Duration::ZERO)?,
            ..RefreshOptions::default()
        },
    };

    let started = service
        .start_refresh_job(request, now, ResetHour::new(14)?)
        .await?;
    assert!(started.started);
    assert_eq!(
        started.job.as_ref().map(|job| job.status),
        Some(IdentityJobStatus::Running)
    );

    let coalesced = service
        .start_refresh_job_with_client(
            client(override_url.clone())?,
            RefreshJobRequest {
                policy: RefreshPolicy::Force,
                ..request
            },
            now,
            ResetHour::new(14)?,
        )
        .await?;
    assert!(!coalesced.started);
    let completed = wait_for_terminal_job(&service).await?;
    assert_eq!(completed.status, IdentityJobStatus::Completed);
    assert_eq!(completed.progress.processed_groups, 1);
    assert_eq!(completed.stats.map(|stats| stats.unique_users), Some(2));
    assert!(override_requests.try_recv().is_err());

    let fresh = service
        .start_refresh_job(request, OffsetDateTime::now_utc(), ResetHour::new(14)?)
        .await?;
    assert!(!fresh.started);

    let forced = service
        .start_refresh_job_with_client(
            client(override_url)?,
            RefreshJobRequest {
                policy: RefreshPolicy::Force,
                ..request
            },
            OffsetDateTime::now_utc(),
            ResetHour::new(14)?,
        )
        .await?;
    assert!(forced.started);
    assert_eq!(
        wait_for_terminal_job(&service).await?.status,
        IdentityJobStatus::Completed
    );
    assert_eq!(drain_requests(&mut default_requests).await.len(), 3);
    assert_eq!(drain_requests(&mut override_requests).await.len(), 3);
    store.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_direct_refresh_is_broadcast_to_all_followers() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let old_time = timestamp(2026, Month::August, 17, 14, 0)?;
    let previous = store
        .replace_identity_snapshot(&snapshot(old_time, "Old Card", "10001", "20001")?)
        .await?;
    let responses = vec![
        MockResponse {
            status: 200,
            body: json!([{"user_id": 10002, "nickname": "New"}]).to_string(),
            delay: Duration::from_millis(50),
        },
        ok(json!([{"group_id": 20002, "group_name": "New Group"}])),
        MockResponse {
            status: 500,
            body: json!({"detail": "failure"}).to_string(),
            delay: Duration::ZERO,
        },
    ];
    let (base_url, mut requests) = mock_server(responses).await?;
    let service = Arc::new(IdentityService::new(store.clone(), client(base_url)?));
    let options = RefreshOptions {
        group_delay: GroupDelay::new(Duration::ZERO)?,
        ..RefreshOptions::default()
    };
    let first_service = Arc::clone(&service);
    let second_service = Arc::clone(&service);
    let fetched_at = OffsetDateTime::now_utc();
    let first = tokio::spawn(async move { first_service.refresh(options, fetched_at).await });
    let second = tokio::spawn(async move { second_service.refresh(options, fetched_at).await });

    assert!(first.await?.is_err());
    assert!(second.await?.is_err());
    assert_eq!(
        store
            .identity_metadata()
            .await?
            .map(|value| value.generation),
        Some(previous.generation)
    );
    assert_eq!(drain_requests(&mut requests).await.len(), 3);
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn background_failure_keeps_snapshot_and_persists_sanitized_error() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let previous = store
        .replace_identity_snapshot(&snapshot(
            timestamp(2026, Month::August, 17, 14, 0)?,
            "Old Card",
            "10001",
            "20001",
        )?)
        .await?;
    let responses = vec![
        ok(json!([{"user_id": 10002, "nickname": "New"}])),
        ok(json!([{"group_id": 20002, "group_name": "New Group"}])),
        MockResponse {
            status: 500,
            body: json!({"token": "SECRET_SENTINEL", "detail": "safe"}).to_string(),
            delay: Duration::ZERO,
        },
    ];
    let (base_url, _) = mock_server(responses).await?;
    let service = IdentityService::new(store.clone(), client(base_url)?);
    let launch = service
        .start_refresh_job(
            RefreshJobRequest {
                policy: RefreshPolicy::Force,
                options: RefreshOptions {
                    group_delay: GroupDelay::new(Duration::ZERO)?,
                    ..RefreshOptions::default()
                },
            },
            OffsetDateTime::now_utc(),
            ResetHour::new(14)?,
        )
        .await?;
    assert!(launch.started);
    let failed = wait_for_terminal_job(&service).await?;
    assert_eq!(failed.status, IdentityJobStatus::Failed);
    let error = failed
        .error
        .ok_or_else(|| io::Error::other("failed job missing error"))?;
    assert_eq!(error.status, Some(500));
    let body = error
        .body
        .ok_or_else(|| io::Error::other("failed job missing sanitized body"))?;
    assert!(!body.contains("SECRET_SENTINEL"));
    assert!(body.contains("[REDACTED]"));
    assert_eq!(
        store
            .identity_metadata()
            .await?
            .map(|value| value.generation),
        Some(previous.generation)
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn first_service_query_marks_a_persisted_running_job_interrupted() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("identity.db")).await?;
    let started = store
        .start_identity_job(
            IdentityRefreshReason::StaleOrMissing,
            timestamp(2026, Month::August, 18, 1, 0)?,
        )
        .await?;
    let generation = match started {
        IdentityJobStart::Started(job) => job.generation,
        IdentityJobStart::AlreadyRunning(_) => {
            return Err(io::Error::other("first job start was unexpectedly coalesced").into());
        }
    };
    let duplicate = store
        .start_identity_job(
            IdentityRefreshReason::ForceRefresh,
            timestamp(2026, Month::August, 18, 1, 30)?,
        )
        .await?;
    assert!(matches!(
        duplicate,
        IdentityJobStart::AlreadyRunning(ref job) if job.generation == generation
    ));
    let service = IdentityService::new(store.clone(), unused_client()?);

    let job = service
        .identity_job_status(timestamp(2026, Month::August, 18, 2, 0)?)
        .await?
        .ok_or_else(|| io::Error::other("interrupted job missing"))?;
    assert_eq!(job.status, IdentityJobStatus::Interrupted);
    assert_eq!(
        job.finished_at,
        Some(timestamp(2026, Month::August, 18, 2, 0)?)
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn terminal_write_failure_is_surfaced_then_retried() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("identity.db");
    let store = StateStore::open(&database).await?;
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database)).await?;
    sqlx::query(
        r#"
        CREATE TRIGGER reject_identity_job_completion
        BEFORE UPDATE OF status ON identity_refresh_job
        WHEN NEW.status = 'completed'
        BEGIN
            SELECT RAISE(FAIL, 'terminal write fixture');
        END
        "#,
    )
    .execute(&mut connection)
    .await?;
    connection.close().await?;
    let (base_url, _) = mock_server(success_responses(Duration::ZERO)).await?;
    let service = IdentityService::new(store.clone(), client(base_url)?);
    service
        .start_refresh_job(
            RefreshJobRequest {
                policy: RefreshPolicy::Force,
                options: RefreshOptions {
                    group_delay: GroupDelay::new(Duration::ZERO)?,
                    ..RefreshOptions::default()
                },
            },
            OffsetDateTime::now_utc(),
            ResetHour::new(14)?,
        )
        .await?;
    sleep(Duration::from_millis(100)).await;

    assert!(
        service
            .identity_job_status(OffsetDateTime::now_utc())
            .await
            .is_err()
    );
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database)).await?;
    sqlx::query("DROP TRIGGER reject_identity_job_completion")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let recovered = service
        .identity_job_status(OffsetDateTime::now_utc())
        .await?
        .ok_or_else(|| io::Error::other("recovered terminal job missing"))?;
    assert_eq!(recovered.status, IdentityJobStatus::Completed);
    store.close().await;
    Ok(())
}

#[test]
fn refresh_parameter_types_enforce_legacy_bounds() -> TestResult {
    assert!(GroupDelay::new(Duration::from_secs(10)).is_ok());
    assert!(GroupDelay::new(Duration::from_millis(10_001)).is_err());
    assert!(MaxGroups::new(1).is_ok());
    assert!(MaxGroups::new(0).is_err());
    assert!(MaxResults::new(1).is_ok());
    assert!(MaxResults::new(20).is_ok());
    assert!(MaxResults::new(21).is_err());
    assert!(ResetHour::new(23).is_ok());
    assert!(ResetHour::new(24).is_err());
    Ok(())
}

async fn wait_for_terminal_job(
    service: &IdentityService,
) -> Result<IdentityJob, Box<dyn Error + Send + Sync>> {
    timeout(Duration::from_secs(3), async {
        loop {
            let job = service
                .identity_job_status(OffsetDateTime::now_utc())
                .await?
                .ok_or_else(|| io::Error::other("identity job missing"))?;
            if job.status != IdentityJobStatus::Running {
                return Ok::<IdentityJob, Box<dyn Error + Send + Sync>>(job);
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "identity job did not finish"))?
}

async fn drain_requests(receiver: &mut mpsc::Receiver<String>) -> Vec<String> {
    let mut requests = Vec::new();
    while let Some(request) = receiver.recv().await {
        requests.push(request);
    }
    requests
}

fn success_responses(delay: Duration) -> Vec<MockResponse> {
    vec![
        MockResponse {
            status: 200,
            body: json!({
                "status": "ok",
                "retcode": 0,
                "data": [{
                    "user_id": 10001,
                    "nickname": "Alice",
                    "remark": "do-not-save"
                }]
            })
            .to_string(),
            delay,
        },
        ok(json!([{
            "group_id": 20001,
            "group_name": "Mai Group",
            "member_count": 2
        }])),
        ok(json!({
            "status": "ok",
            "retcode": 0,
            "data": [
                {"user_id": 10001, "nickname": "Alice", "card": "Captain"},
                {"user_id": 10002, "nickname": "Bob", "card": ""}
            ]
        })),
    ]
}

fn snapshot(
    fetched_at: OffsetDateTime,
    card: &str,
    qq: &str,
    group_id: &str,
) -> Result<IdentitySnapshot, maimai_core::ValidationError> {
    Ok(IdentitySnapshot {
        fetched_at,
        friends: Vec::new(),
        groups: vec![IdentityGroupSnapshot {
            group_id: GroupId::new(group_id)?,
            group_name: Some("Old Group".to_owned()),
            member_count: Some(1),
            members: vec![member(qq, "Old", card)?],
        }],
    })
}

fn member(
    qq: &str,
    nickname: &str,
    card: &str,
) -> Result<IdentitySnapshotMember, maimai_core::ValidationError> {
    Ok(IdentitySnapshotMember {
        qq: QqId::new(qq)?,
        nickname: Some(nickname.to_owned()),
        card: Some(card.to_owned()),
    })
}

fn timestamp(
    year: i32,
    month: Month,
    day: u8,
    hour: u8,
    minute: u8,
) -> Result<OffsetDateTime, time::error::ComponentRange> {
    Ok(PrimitiveDateTime::new(
        Date::from_calendar_date(year, month, day)?,
        Time::from_hms(hour, minute, 0)?,
    )
    .assume_utc())
}

fn client(base_url: Url) -> Result<NapCatClient, maimai_providers::NapCatError> {
    NapCatClient::new(NapCatConfig::new(base_url, Duration::from_secs(2), None)?)
}

fn unused_client() -> Result<NapCatClient, Box<dyn Error + Send + Sync>> {
    Ok(client(Url::parse("http://127.0.0.1:9/")?)?)
}

fn ok(body: Value) -> MockResponse {
    MockResponse {
        status: 200,
        body: body.to_string(),
        delay: Duration::ZERO,
    }
}

async fn mock_server(
    responses: Vec<MockResponse>,
) -> Result<(Url, mpsc::Receiver<String>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (request_tx, request_rx) = mpsc::channel(responses.len().max(1));
    tokio::spawn(async move {
        for response in responses {
            if serve_once(&listener, response, &request_tx).await.is_err() {
                break;
            }
        }
    });
    Ok((
        Url::parse(&format!("http://{address}/onebot/"))?,
        request_rx,
    ))
}

async fn serve_once(
    listener: &TcpListener,
    response: MockResponse,
    request_tx: &mpsc::Sender<String>,
) -> Result<(), io::Error> {
    let (mut stream, _) = listener.accept().await?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "mock request ended before headers",
            ));
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let _ = request_tx
        .send(String::from_utf8_lossy(&request).into_owned())
        .await;
    sleep(response.delay).await;
    let reason = if response.status >= 400 {
        "Error"
    } else {
        "OK"
    };
    let encoded = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        response.body.len(),
        response.body
    );
    stream.write_all(encoded.as_bytes()).await?;
    stream.shutdown().await
}
