use std::{error::Error, fs, io, sync::Arc, time::Duration};

use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, GroupId, QqId,
    RatingBreakdown, ScoreSource, SongIdNamespace, SourceSongId,
};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, NapCatClient, NapCatConfig,
    OAuthConfig, lxns_score::LxnsScoreEndpoint,
};
use maimai_storage::{
    CachedB50Entry, CachedChart, CachedFitIndex, CachedFitIndexSection, CachedPlayer,
    PlayerB50Snapshot, PlayerProfile, PlayerRecord, RankingJobStart, RankingJobStatus,
    RankingMember, RankingNamespace, RankingRefreshReason, RankingSnapshot, StateStore,
};
use serde_json::json;
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::{
    io::AsyncWriteExt,
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use url::Url;

use crate::{oauth::OAuthService, score_service::PlayerScoreService};

use super::{RankingService, RefreshOptions, SongReportOptions, SongSort, SongTarget, SortOrder};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn shutdown_awaits_later_tasks_after_an_earlier_terminal_failure() -> TestResult {
    let fixture = fixture().await?;
    let service = service(&fixture, Url::parse("http://127.0.0.1:9/")?)?;
    let group = GroupId::new("shutdown-drain")?;
    let now = OffsetDateTime::now_utc();
    let job = fixture
        .store
        .start_ranking_job(
            RankingNamespace::B50,
            &group,
            RankingRefreshReason::Miss,
            now,
        )
        .await?;
    let RankingJobStart::Started(job) = job else {
        return Err("shutdown fixture job did not start".into());
    };

    let aborted = tokio::spawn(async { Ok::<(), maimai_storage::RankingJobError>(()) });
    aborted.abort();
    let store = fixture.store.clone();
    let persisted_group = group.clone();
    let second = tokio::spawn(async move {
        let error = super::RankingError::Task.safe_job_error();
        let persisted = store
            .fail_ranking_job(
                RankingNamespace::B50,
                &persisted_group,
                job.generation,
                now,
                &error,
            )
            .await
            .map_err(|_| error.clone())?;
        if persisted { Ok(()) } else { Err(error) }
    });
    {
        let mut state = service.state.lock().await;
        state
            .active
            .insert((RankingNamespace::B50, String::new()), aborted);
        state
            .active
            .insert((RankingNamespace::B50, group.as_str().to_owned()), second);
    }

    assert!(service.shutdown(now).await.is_err());
    assert!(service.state.lock().await.active.is_empty());
    let terminal = fixture
        .store
        .ranking_job(RankingNamespace::B50, &group)
        .await?
        .ok_or("shutdown terminal missing")?;
    assert_eq!(terminal.status, RankingJobStatus::Failed);
    Ok(())
}

#[tokio::test]
async fn concurrent_ensure_starts_one_job_and_recovery_does_not_interrupt_it() -> TestResult {
    let fixture = fixture().await?;
    let (napcat_url, mut requests, release) = slow_napcat().await?;
    let service = service(&fixture, napcat_url)?.with_clock(fixed_completion_time);
    let group = GroupId::new("123")?;
    let now = OffsetDateTime::UNIX_EPOCH + time::Duration::hours(13) + time::Duration::minutes(59);
    let (first, second) = tokio::join!(
        service.ensure_cache(
            RankingNamespace::B50,
            group.clone(),
            false,
            RefreshOptions::default(),
            now,
        ),
        service.ensure_cache(
            RankingNamespace::B50,
            group.clone(),
            false,
            RefreshOptions::default(),
            now,
        ),
    );
    let first = first?.ok_or("first launch missing")?;
    let second = second?.ok_or("second launch missing")?;
    assert_ne!(first.started, second.started);
    let running = fixture
        .store
        .ranking_job(RankingNamespace::B50, &group)
        .await?
        .ok_or("running job missing")?;
    assert_eq!(running.status, RankingJobStatus::Running);
    assert_eq!(requests.recv().await.ok_or("NapCat request missing")?, 1);
    release
        .send(())
        .map_err(|_| io::Error::other("release failed"))?;
    wait_for_terminal(&service, &group).await?;
    let completed = fixture
        .store
        .ranking_job(RankingNamespace::B50, &group)
        .await?
        .ok_or("completed job missing")?;
    assert_eq!(completed.status, RankingJobStatus::Completed);
    let cache = fixture
        .store
        .ranking_cache(RankingNamespace::B50, &group)
        .await?
        .ok_or("completed cache missing")?;
    assert_eq!(cache.snapshot.fetched_at, fixed_completion_time());
    assert_eq!(
        cache.snapshot.next_reset_at,
        OffsetDateTime::UNIX_EPOCH + time::Duration::days(1) + time::Duration::hours(14)
    );
    Ok(())
}

#[tokio::test]
async fn song_sort_keeps_exact_99_9_below_100_0() -> TestResult {
    let fixture = fixture().await?;
    let service = service(&fixture, Url::parse("http://127.0.0.1:9/")?)?;
    let group = GroupId::new("song")?;
    let now = OffsetDateTime::now_utc();
    let job = fixture
        .store
        .start_ranking_job(
            RankingNamespace::SongScore,
            &group,
            RankingRefreshReason::Miss,
            now,
        )
        .await?;
    let RankingJobStart::Started(job) = job else {
        return Err("job did not start".into());
    };
    let members = [member(0, "10001")?, member(1, "10002")?];
    let records = [
        (members[0].qq.clone(), chart("99.9000")?),
        (members[1].qq.clone(), chart("100.0000")?),
    ];
    fixture
        .store
        .complete_song_ranking(
            &snapshot(&group, job.generation, now),
            &members,
            &records,
            now,
        )
        .await?;
    let report = service
        .song_cached_report(
            &group,
            SongReportOptions {
                target: SongTarget {
                    title: "Link".to_owned(),
                    ids: vec![SourceSongId::numeric(SongIdNamespace::DivingFish, 383)],
                    difficulty: Some(Difficulty::Master),
                    deluxe: Some(false),
                },
                sort: SongSort::Achievements,
                order: SortOrder::Descending,
                achievements_min: None,
                achievements_max: None,
                window: Default::default(),
            },
            now,
        )
        .await?;
    assert_eq!(report.rows[0].qq.as_str(), "10002");
    assert_eq!(report.rows[1].qq.as_str(), "10001");
    Ok(())
}

#[tokio::test]
async fn failed_force_refresh_preserves_previous_snapshot() -> TestResult {
    let fixture = fixture().await?;
    let group = GroupId::new("preserve")?;
    let now = OffsetDateTime::now_utc();
    let old_job = fixture
        .store
        .start_ranking_job(
            RankingNamespace::B50,
            &group,
            RankingRefreshReason::Miss,
            now,
        )
        .await?;
    let RankingJobStart::Started(old_job) = old_job else {
        return Err("old job did not start".into());
    };
    let old_snapshot = RankingSnapshot {
        namespace: RankingNamespace::B50,
        group_id: group.clone(),
        generation: old_job.generation,
        fetched_at: now,
        next_reset_at: now + time::Duration::days(1),
        member_count: 1,
        success_count: 1,
        failure_count: 0,
        skipped_count: 0,
        cache_hit_count: 0,
        shared_fetch_count: 0,
    };
    fixture
        .store
        .complete_b50_ranking(
            &old_snapshot,
            &[CachedB50Entry {
                member: member(0, "10001")?,
                player: CachedPlayer {
                    rating: Some(15000),
                    ..CachedPlayer::default()
                },
                rating_breakdown: maimai_core::RatingBreakdown {
                    b35: 10000,
                    b15: 5000,
                    total: 15000,
                },
                fit_index: CachedFitIndex::default(),
                charts: Vec::new(),
            }],
            now,
        )
        .await?;

    let napcat_url = napcat_once(r#"{"status":"ok","retcode":0,"data":[{"group_id":"preserve","user_id":"10001","nickname":"member","card":""}]}"#).await?;
    let service = service(&fixture, napcat_url)?;
    let launch = service
        .ensure_cache(
            RankingNamespace::B50,
            group.clone(),
            true,
            RefreshOptions::default(),
            now,
        )
        .await?
        .ok_or("force launch missing")?;
    assert!(launch.started);
    wait_for_terminal(&service, &group).await?;
    let failed = fixture
        .store
        .ranking_job(RankingNamespace::B50, &group)
        .await?
        .ok_or("failed job missing")?;
    assert_eq!(failed.status, RankingJobStatus::Failed);
    let preserved = fixture
        .store
        .ranking_cache(RankingNamespace::B50, &group)
        .await?
        .ok_or("old cache disappeared")?;
    assert_eq!(preserved.snapshot.generation, old_snapshot.generation);
    Ok(())
}

#[tokio::test]
async fn b50_refresh_mixes_shared_hits_and_network_in_member_order() -> TestResult {
    let fixture = fixture().await?;
    let now = OffsetDateTime::UNIX_EPOCH + time::Duration::days(10) + time::Duration::hours(15);
    seed_b50(&fixture.store, "10001", now, 15_001).await?;
    seed_b50(&fixture.store, "10003", now, 15_003).await?;
    let napcat = napcat_once(
        r#"{"status":"ok","retcode":0,"data":[
        {"group_id":"mixed","user_id":"10001","nickname":"one","card":""},
        {"group_id":"mixed","user_id":"10002","nickname":"two","card":""},
        {"group_id":"mixed","user_id":"10003","nickname":"three","card":""}]}"#,
    )
    .await?;
    let (df, mut requests) = df_b50_server(1).await?;
    let service = service_with_df(&fixture, napcat, df)?.with_clock(fixed_completion_time);
    let group = GroupId::new("mixed")?;
    let launch = service
        .ensure_cache(
            RankingNamespace::B50,
            group.clone(),
            false,
            RefreshOptions::default(),
            now,
        )
        .await?
        .ok_or("launch missing")?;
    assert!(launch.started);
    wait_for_terminal_namespace(&service, &group, RankingNamespace::B50).await?;
    assert_eq!(requests.recv().await, Some(1));
    let cache = fixture
        .store
        .ranking_cache(RankingNamespace::B50, &group)
        .await?
        .ok_or("B50 ranking missing")?;
    assert_eq!(cache.snapshot.cache_hit_count, 2);
    let maimai_storage::RankingSnapshotData::B50(entries) = cache.data else {
        return Err("wrong cache namespace".into());
    };
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.member.qq.as_str())
            .collect::<Vec<_>>(),
        ["10001", "10002", "10003"]
    );
    Ok(())
}

#[tokio::test]
async fn stale_b50_shared_cache_falls_back_to_network() -> TestResult {
    let fixture = fixture().await?;
    let now = OffsetDateTime::UNIX_EPOCH + time::Duration::days(10) + time::Duration::hours(15);
    seed_b50(
        &fixture.store,
        "10001",
        now - time::Duration::days(2),
        14_000,
    )
    .await?;
    let napcat = napcat_once(
        r#"{"status":"ok","retcode":0,"data":[
        {"group_id":"stale","user_id":"10001","nickname":"one","card":""}]}"#,
    )
    .await?;
    let (df, mut requests) = df_b50_server(1).await?;
    let service = service_with_df(&fixture, napcat, df)?;
    let group = GroupId::new("stale")?;
    service
        .ensure_cache(
            RankingNamespace::B50,
            group.clone(),
            false,
            RefreshOptions::default(),
            now,
        )
        .await?;
    wait_for_terminal_namespace(&service, &group, RankingNamespace::B50).await?;
    assert_eq!(requests.recv().await, Some(1));
    let cache = fixture
        .store
        .ranking_cache(RankingNamespace::B50, &group)
        .await?
        .ok_or("ranking missing")?;
    assert_eq!(cache.snapshot.cache_hit_count, 0);
    Ok(())
}

#[tokio::test]
async fn corrupted_b50_shared_cache_is_a_network_fallback() -> TestResult {
    let fixture = fixture().await?;
    let now = OffsetDateTime::UNIX_EPOCH + time::Duration::days(10) + time::Duration::hours(15);
    seed_b50(&fixture.store, "10001", now, 15_000).await?;
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        fixture._temp.path().join("state.db").display()
    ))
    .await?;
    sqlx::query("UPDATE player_b50_cache SET metadata_quality = 0 WHERE qq = '10001'")
        .execute(&pool)
        .await?;
    pool.close().await;
    let napcat = napcat_once(
        r#"{"status":"ok","retcode":0,"data":[
        {"group_id":"corrupt","user_id":"10001","nickname":"one","card":""}]}"#,
    )
    .await?;
    let (df, mut requests) = df_b50_server(1).await?;
    let service = service_with_df(&fixture, napcat, df)?;
    let group = GroupId::new("corrupt")?;
    service
        .ensure_cache(
            RankingNamespace::B50,
            group.clone(),
            false,
            RefreshOptions::default(),
            now,
        )
        .await?;
    wait_for_terminal_namespace(&service, &group, RankingNamespace::B50).await?;
    assert_eq!(requests.recv().await, Some(1));
    Ok(())
}

#[tokio::test]
async fn song_refresh_uses_fresh_complete_df_snapshot_without_network() -> TestResult {
    let fixture = fixture().await?;
    let now = OffsetDateTime::UNIX_EPOCH + time::Duration::days(10) + time::Duration::hours(15);
    seed_full_scores(&fixture.store, "10001", now).await?;
    let napcat = napcat_once(
        r#"{"status":"ok","retcode":0,"data":[
        {"group_id":"song-hit","user_id":"10001","nickname":"one","card":""}]}"#,
    )
    .await?;
    let service = service(&fixture, napcat)?;
    let group = GroupId::new("song-hit")?;
    service
        .ensure_cache(
            RankingNamespace::SongScore,
            group.clone(),
            false,
            RefreshOptions::default(),
            now,
        )
        .await?;
    wait_for_terminal_namespace(&service, &group, RankingNamespace::SongScore).await?;
    let cache = fixture
        .store
        .ranking_cache(RankingNamespace::SongScore, &group)
        .await?
        .ok_or("song ranking missing")?;
    assert_eq!(cache.snapshot.cache_hit_count, 1);
    assert_eq!(cache.snapshot.success_count, 1);
    Ok(())
}

struct Fixture {
    _temp: TempDir,
    store: StateStore,
    catalog: Arc<CatalogStore>,
}

async fn fixture() -> Result<Fixture, Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    write_catalog(&temp)?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let catalog = Arc::new(CatalogStore::load(CatalogFiles::from_data_dir(temp.path())).await?);
    Ok(Fixture {
        _temp: temp,
        store,
        catalog,
    })
}

fn service(
    fixture: &Fixture,
    napcat_url: Url,
) -> Result<RankingService, Box<dyn Error + Send + Sync>> {
    service_with_df(fixture, napcat_url, Url::parse("http://127.0.0.1:9/")?)
}

fn service_with_df(
    fixture: &Fixture,
    napcat_url: Url,
    df_url: Url,
) -> Result<RankingService, Box<dyn Error + Send + Sync>> {
    let napcat = NapCatClient::new(NapCatConfig::new(napcat_url, Duration::from_secs(2), None)?)?;
    let identity = crate::identity::IdentityService::new(fixture.store.clone(), napcat.clone());
    let df = DivingFishScoreClient::new(DivingFishClient::with_base_urls(
        df_url.join("api/")?.as_str(),
        df_url.join("covers/")?.as_str(),
    )?);
    let oauth_config = OAuthConfig::new(
        "client-id",
        None,
        None,
        Url::parse("http://127.0.0.1:9/oauth/authorize")?,
        Url::parse("http://127.0.0.1:9/oauth/token")?,
        vec!["read_player".to_owned()],
    )?;
    let scores = Arc::new(PlayerScoreService::with_lxns(
        fixture.store.clone(),
        Arc::clone(&fixture.catalog),
        df,
        OAuthService::new(fixture.store.clone(), LxnsOAuthClient::new(oauth_config)?),
        LxnsScoreEndpoint::new(
            Url::parse("http://127.0.0.1:9/api/v0/")?,
            Duration::from_secs(1),
        )?,
    ));
    Ok(RankingService::new(
        fixture.store.clone(),
        napcat,
        identity,
        scores,
        Arc::clone(&fixture.catalog),
    ))
}

async fn slow_napcat()
-> Result<(Url, mpsc::Receiver<u8>, oneshot::Sender<()>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (request_tx, request_rx) = mpsc::channel(1);
    let (release_tx, release_rx) = oneshot::channel();
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let mut buffer = [0_u8; 4096];
        let _ = stream.readable().await;
        let _ = stream.try_read(&mut buffer);
        let _ = request_tx.send(1).await;
        let _ = release_rx.await;
        let body = r#"{"status":"ok","retcode":0,"data":[]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });
    Ok((
        Url::parse(&format!("http://{address}/"))?,
        request_rx,
        release_tx,
    ))
}

async fn napcat_once(body: &'static str) -> Result<Url, Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let mut buffer = [0_u8; 4096];
        let _ = stream.readable().await;
        let _ = stream.try_read(&mut buffer);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });
    Ok(Url::parse(&format!("http://{address}/"))?)
}

async fn df_b50_server(
    response_count: usize,
) -> Result<(Url, mpsc::Receiver<u8>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(response_count.max(1));
    tokio::spawn(async move {
        for _ in 0..response_count {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let mut buffer = [0_u8; 4096];
            let _ = stream.readable().await;
            let _ = stream.try_read(&mut buffer);
            let _ = sender.send(1).await;
            let body = json!({"nickname":"Network","rating":15002,"charts":{"sd":[{
                "song_id":383,"title":"Link","type":"SD","level":"13","level_index":3,
                "ds":"13.0","achievements":"100.0000","ra":280}],"dx":[]}})
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body,
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    Ok((Url::parse(&format!("http://{address}/"))?, receiver))
}

async fn seed_b50(
    store: &StateStore,
    qq: &str,
    fetched_at: OffsetDateTime,
    rating: u32,
) -> TestResult {
    let snapshot = PlayerB50Snapshot::new(
        QqId::new(qq)?,
        ScoreSource::DivingFish,
        fetched_at,
        CachedPlayer {
            nickname: Some(format!("Cached {qq}")),
            rating: Some(rating),
            ..CachedPlayer::default()
        },
        RatingBreakdown {
            b35: rating,
            b15: 0,
            total: rating,
        },
        CachedFitIndex {
            b50: CachedFitIndexSection {
                virtual_rating: Some(1),
                counted: 1,
                total_rating: Some(u64::from(rating)),
                ..CachedFitIndexSection::default()
            },
            ..CachedFitIndex::default()
        },
        Vec::new(),
    )?;
    store
        .replace_player_b50_snapshot(&snapshot, OffsetDateTime::UNIX_EPOCH)
        .await?;
    Ok(())
}

async fn seed_full_scores(store: &StateStore, qq: &str, fetched_at: OffsetDateTime) -> TestResult {
    let qq = QqId::new(qq)?;
    let updated_at = fetched_at.format(&time::format_description::well_known::Rfc3339)?;
    let profile = PlayerProfile {
        qq: qq.clone(),
        nickname: Some("Cached Full".to_owned()),
        player_rating: Some(15_000),
        player_old_rating: None,
        player_new_rating: None,
        score_source: Some(ScoreSource::DivingFish),
        source_detail: Some("diving_fish_full_snapshot".to_owned()),
        raw: None,
        updated_at: updated_at.clone(),
    };
    let record = PlayerRecord {
        qq,
        chart: ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::DivingFish, 383),
            ChartGeneration::Standard,
            Difficulty::Master,
        )?,
        title: "Link".to_owned(),
        level: Some("13".to_owned()),
        level_label: Some("Master".to_owned()),
        ds: Some(ChartConstant::from_decimal_str("13.0")?),
        achievements: Some(AchievementRate::from_decimal_str("100.0000")?.into()),
        dx_score: Some(1_000),
        fc: None,
        fs: None,
        rate: Some("sss".to_owned()),
        ra: Some(280),
        version: Some("Current".to_owned()),
        is_new: true,
        score_source: ScoreSource::DivingFish,
        source_detail: None,
        raw: None,
        payload: json!({}),
        updated_at,
    };
    store
        .replace_player_score_snapshot(&profile, &[record])
        .await?;
    Ok(())
}

async fn wait_for_terminal(service: &RankingService, group: &GroupId) -> TestResult {
    wait_for_terminal_namespace(service, group, RankingNamespace::B50).await
}

async fn wait_for_terminal_namespace(
    service: &RankingService,
    group: &GroupId,
    namespace: RankingNamespace,
) -> TestResult {
    for _ in 0..100 {
        if let Some(job) = service
            .job_status(namespace, group, OffsetDateTime::now_utc())
            .await?
            && job.status != RankingJobStatus::Running
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err("job did not finish".into())
}

fn snapshot(group: &GroupId, generation: u64, now: OffsetDateTime) -> RankingSnapshot {
    RankingSnapshot {
        namespace: RankingNamespace::SongScore,
        group_id: group.clone(),
        generation,
        fetched_at: now,
        next_reset_at: now + time::Duration::days(1),
        member_count: 2,
        success_count: 2,
        failure_count: 0,
        skipped_count: 0,
        cache_hit_count: 0,
        shared_fetch_count: 0,
    }
}

fn member(ordinal: u32, qq: &str) -> Result<RankingMember, maimai_core::ValidationError> {
    Ok(RankingMember {
        ordinal,
        qq: QqId::new(qq)?,
        nickname: Some(qq.to_owned()),
        card: None,
        display_name: qq.to_owned(),
        waterfish_nickname: None,
        waterfish_username: None,
    })
}

fn chart(achievements: &str) -> Result<CachedChart, Box<dyn Error + Send + Sync>> {
    Ok(CachedChart {
        key: ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::DivingFish, 383),
            ChartGeneration::Standard,
            Difficulty::Master,
        )?,
        title: "Link".to_owned(),
        level: "13".to_owned(),
        constant: None,
        achievements: Some(AchievementRate::from_decimal_str(achievements)?),
        dx_score: None,
        rating: Some(300),
        original_rating: None,
        grade: Some("sss".to_owned()),
        full_combo: None,
        full_sync: None,
        version: "PRiSM".to_owned(),
        is_current: false,
        fit_constant: None,
    })
}

fn write_catalog(temp: &TempDir) -> Result<(), io::Error> {
    let files = [
        (
            "lxns_song_list.json",
            json!({"songs":[{"id":383,"title":"Link","artist":"A","genre":"maimai",
                "bpm":150,"version":25000,"difficulties":{"standard":[{
                    "difficulty":3,"level":"13","level_value":13.0,"notes":{}}]}}],
                "genres":[],"versions":[{"title":"Current","version":25000}]})
            .to_string(),
        ),
        (
            "divingfish_song_list.json",
            json!([{"id":"383","title":"Link","type":"SD",
            "ds":[1,2,3,13.0],"level":["1","2","3","13"],"charts":[{},{},{},{}],
            "basic_info":{"bpm":150,"from":"Current","is_new":true}}])
            .to_string(),
        ),
        ("lxns_alias_list.json", r#"{"aliases":[]}"#.to_owned()),
        ("music_alias.json", r#"{"content":[]}"#.to_owned()),
        ("custom_aliases.json", "{}".to_owned()),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#.to_owned()),
        ("zh_s2t.json", "{}".to_owned()),
        (
            "divingfish_chart_stats.json",
            json!({"charts":{"383":[{},{},{},{
            "fit_diff":13.1}]}})
            .to_string(),
        ),
    ];
    for (name, contents) in files {
        fs::write(temp.path().join(name), contents)?;
    }
    Ok(())
}

fn fixed_completion_time() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + time::Duration::hours(14) + time::Duration::minutes(1)
}
