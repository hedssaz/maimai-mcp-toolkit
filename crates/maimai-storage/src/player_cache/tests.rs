use std::error::Error;

use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, QqId, RatingBreakdown,
    ScoreSource, SongIdNamespace, SourceSongId,
};
use tempfile::TempDir;
use time::{Duration, OffsetDateTime};
use tokio::sync::Barrier;

use crate::{
    B50CacheWriteOutcome, B50Section, CachedB50Chart, CachedChart, CachedFitIndex,
    CachedFitIndexSection, CachedPlayer, FullScoreSnapshotWriteOutcome, PlayerB50Snapshot,
    PlayerProfile, PlayerRecord, StateStore,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn b50_cache_roundtrips_across_reopen_and_honors_freshness_and_source() -> TestResult {
    let temp = TempDir::new()?;
    let path = temp.path().join("state.db");
    let store = StateStore::open(&path).await?;
    let fetched_at = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
    let snapshot = b50_snapshot("10001", fetched_at, 15_000, true)?;
    assert_eq!(
        store
            .replace_player_b50_snapshot(&snapshot, OffsetDateTime::UNIX_EPOCH)
            .await?,
        B50CacheWriteOutcome::Written
    );
    store.close().await;

    let reopened = StateStore::open(&path).await?;
    let qq = QqId::new("10001")?;
    let loaded = reopened
        .player_b50_snapshot(&qq, fetched_at)
        .await?
        .ok_or("cache missing after reopen")?;
    assert_eq!(loaded, snapshot);
    assert!(
        reopened
            .player_b50_snapshot(&qq, fetched_at + Duration::nanoseconds(1))
            .await?
            .is_none()
    );
    assert!(
        PlayerB50Snapshot::new(
            qq,
            ScoreSource::Lxns,
            fetched_at,
            CachedPlayer::default(),
            RatingBreakdown {
                b35: 0,
                b15: 0,
                total: 0,
            },
            CachedFitIndex::default(),
            Vec::new(),
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn fresh_high_quality_survives_low_quality_and_failed_replacement() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let baseline = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
    let high = b50_snapshot("10001", baseline, 15_000, true)?;
    store
        .replace_player_b50_snapshot(&high, OffsetDateTime::UNIX_EPOCH)
        .await?;
    let low = b50_snapshot("10001", baseline + Duration::hours(1), 15_100, false)?;
    assert_eq!(
        store
            .replace_player_b50_snapshot(&low, baseline - Duration::hours(1))
            .await?,
        B50CacheWriteOutcome::PreservedHigherQuality
    );
    assert_eq!(
        store
            .player_b50_snapshot(&QqId::new("10001")?, OffsetDateTime::UNIX_EPOCH)
            .await?
            .ok_or("high-quality cache missing")?
            .player()
            .rating,
        Some(15_000)
    );

    sqlx::query(
        r#"CREATE TRIGGER reject_bad_b50 BEFORE INSERT ON player_b50_cache
           WHEN NEW.player_rating = 999 BEGIN SELECT RAISE(FAIL, 'fixture failure'); END"#,
    )
    .execute(&store.pool)
    .await?;
    let rejected = b50_snapshot("10001", baseline + Duration::days(2), 999, true)?;
    assert!(
        store
            .replace_player_b50_snapshot(&rejected, baseline + Duration::days(1))
            .await
            .is_err()
    );
    assert_eq!(
        store
            .player_b50_snapshot(&QqId::new("10001")?, OffsetDateTime::UNIX_EPOCH)
            .await?
            .ok_or("old cache lost")?
            .player()
            .rating,
        Some(15_000)
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_high_and_low_quality_writers_always_finish_with_high_quality() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let fetched_at = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
    let high = b50_snapshot("10001", fetched_at, 15_000, true)?;
    let low = b50_snapshot("10001", fetched_at, 15_100, false)?;
    let barrier = std::sync::Arc::new(Barrier::new(2));
    let high_store = store.clone();
    let high_barrier = barrier.clone();
    let high_task = tokio::spawn(async move {
        high_barrier.wait().await;
        high_store
            .replace_player_b50_snapshot(&high, OffsetDateTime::UNIX_EPOCH)
            .await
    });
    let low_store = store.clone();
    let low_task = tokio::spawn(async move {
        barrier.wait().await;
        low_store
            .replace_player_b50_snapshot(&low, OffsetDateTime::UNIX_EPOCH)
            .await
    });
    high_task.await??;
    low_task.await??;
    let cached = store
        .player_b50_snapshot(&QqId::new("10001")?, OffsetDateTime::UNIX_EPOCH)
        .await?
        .ok_or("cache missing")?;
    assert!(cached.fit_index().available());
    assert_eq!(cached.player().rating, Some(15_000));
    Ok(())
}

#[tokio::test]
async fn sidecar_tampering_is_rejected_instead_of_trusted() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let fetched_at = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
    let snapshot = b50_snapshot("10001", fetched_at, 15_000, true)?;
    store
        .replace_player_b50_snapshot(&snapshot, OffsetDateTime::UNIX_EPOCH)
        .await?;
    sqlx::query("UPDATE player_b50_cache SET metadata_quality = 0, player_rating = 1 WHERE qq = ?")
        .bind("10001")
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .player_b50_snapshot(&QqId::new("10001")?, OffsetDateTime::UNIX_EPOCH)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn full_score_marker_requires_complete_same_source_rows_and_is_invalidated_by_upsert()
-> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let fetched_at = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
    let timestamp = fetched_at.format(&time::format_description::well_known::Rfc3339)?;
    let qq = QqId::new("10001")?;
    let profile = PlayerProfile {
        qq: qq.clone(),
        nickname: Some("Tester".to_owned()),
        player_rating: Some(300),
        player_old_rating: None,
        player_new_rating: None,
        score_source: Some(ScoreSource::DivingFish),
        source_detail: Some("diving_fish_full_snapshot".to_owned()),
        raw: None,
        updated_at: timestamp.clone(),
    };
    let record = record(qq.clone(), ScoreSource::DivingFish, timestamp.clone())?;
    store
        .replace_player_score_snapshot(&profile, std::slice::from_ref(&record))
        .await?;
    let snapshot = store
        .full_score_snapshot(&qq, ScoreSource::DivingFish, fetched_at)
        .await?
        .ok_or("full snapshot missing")?;
    assert_eq!(snapshot.records().len(), 1);
    assert!(
        store
            .full_score_snapshot(&qq, ScoreSource::Lxns, fetched_at)
            .await?
            .is_none()
    );
    assert!(
        store
            .full_score_snapshot(
                &qq,
                ScoreSource::DivingFish,
                fetched_at + Duration::nanoseconds(1),
            )
            .await?
            .is_none()
    );
    store.upsert_record(&record).await?;
    assert!(
        store
            .full_score_snapshot(&qq, ScoreSource::DivingFish, OffsetDateTime::UNIX_EPOCH)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn older_full_snapshot_cannot_overwrite_newer_completed_snapshot() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let qq = QqId::new("10001")?;
    let later = OffsetDateTime::UNIX_EPOCH + Duration::days(20);
    let later_text = later.format(&time::format_description::well_known::Rfc3339)?;
    let later_profile = PlayerProfile {
        qq: qq.clone(),
        nickname: Some("Later".to_owned()),
        player_rating: Some(500),
        player_old_rating: None,
        player_new_rating: None,
        score_source: Some(ScoreSource::DivingFish),
        source_detail: None,
        raw: None,
        updated_at: later_text.clone(),
    };
    let later_record = record(qq.clone(), ScoreSource::DivingFish, later_text)?;
    assert_eq!(
        store
            .replace_player_score_snapshot(&later_profile, &[later_record])
            .await?,
        FullScoreSnapshotWriteOutcome::Written
    );

    let older = later - Duration::days(1);
    let older_text = older.format(&time::format_description::well_known::Rfc3339)?;
    let older_profile = PlayerProfile {
        qq: qq.clone(),
        nickname: Some("Older".to_owned()),
        player_rating: Some(100),
        updated_at: older_text.clone(),
        ..later_profile
    };
    let older_record = record(qq.clone(), ScoreSource::DivingFish, older_text)?;
    assert_eq!(
        store
            .replace_player_score_snapshot(&older_profile, &[older_record])
            .await?,
        FullScoreSnapshotWriteOutcome::StaleIgnored
    );
    let cached = store
        .full_score_snapshot(&qq, ScoreSource::DivingFish, OffsetDateTime::UNIX_EPOCH)
        .await?
        .ok_or("full snapshot missing")?;
    assert_eq!(cached.fetched_at(), later);
    assert_eq!(cached.profile().nickname.as_deref(), Some("Later"));
    Ok(())
}

fn b50_snapshot(
    qq: &str,
    fetched_at: OffsetDateTime,
    rating: u32,
    high_quality: bool,
) -> Result<PlayerB50Snapshot, Box<dyn Error + Send + Sync>> {
    let fit = if high_quality {
        CachedFitIndex {
            b50: CachedFitIndexSection {
                virtual_rating: Some(1),
                counted: 1,
                total_rating: Some(u64::from(rating)),
                ..CachedFitIndexSection::default()
            },
            ..CachedFitIndex::default()
        }
    } else {
        CachedFitIndex::default()
    };
    let key = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::DivingFish, 383),
        ChartGeneration::Standard,
        Difficulty::Master,
    )?;
    Ok(PlayerB50Snapshot::new(
        QqId::new(qq)?,
        ScoreSource::DivingFish,
        fetched_at,
        CachedPlayer {
            nickname: Some("Tester".to_owned()),
            rating: Some(rating),
            ..CachedPlayer::default()
        },
        RatingBreakdown {
            b35: rating,
            b15: 0,
            total: rating,
        },
        fit,
        vec![CachedB50Chart {
            section: B50Section::B35,
            ordinal: 0,
            chart: CachedChart {
                key,
                title: "Link".to_owned(),
                level: "13".to_owned(),
                constant: Some(ChartConstant::from_decimal_str("13.0")?),
                achievements: Some(AchievementRate::from_decimal_str("100.0000")?),
                dx_score: Some(1_000),
                rating: Some(rating),
                original_rating: None,
                grade: Some("sss".to_owned()),
                full_combo: None,
                full_sync: None,
                version: "Current".to_owned(),
                is_current: false,
                fit_constant: high_quality
                    .then(|| ChartConstant::from_decimal_str("13.1"))
                    .transpose()?,
            },
        }],
    )?)
}

fn record(
    qq: QqId,
    source: ScoreSource,
    updated_at: String,
) -> Result<PlayerRecord, Box<dyn Error + Send + Sync>> {
    Ok(PlayerRecord {
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
        score_source: source,
        source_detail: None,
        raw: None,
        payload: serde_json::json!({}),
        updated_at,
    })
}
