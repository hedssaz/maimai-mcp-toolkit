use std::error::Error;

use maimai_core::{
    AchievementRate, ChartGeneration, ChartKey, Difficulty, GroupId, QqId, RatingBreakdown,
    SongIdNamespace, SourceSongId,
};
use tempfile::TempDir;
use time::{Duration, OffsetDateTime};

use super::{
    B50Section, CachedB50Chart, CachedB50Entry, CachedChart, CachedPlayer, RankingJobStart,
    RankingJobStatus, RankingMember, RankingNamespace, RankingRefreshReason, RankingSnapshot,
    RankingSnapshotData,
};
use crate::StateStore;

#[tokio::test]
async fn b50_and_song_snapshots_are_isolated_and_foreign_keys_hold()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let group = GroupId::new("group-1")?;
    let now = OffsetDateTime::now_utc();
    let b50_job = store
        .start_ranking_job(
            RankingNamespace::B50,
            &group,
            RankingRefreshReason::Miss,
            now,
        )
        .await?;
    let RankingJobStart::Started(b50_job) = b50_job else {
        return Err("expected b50 job start".into());
    };
    let member = member(0, "10001")?;
    let entry = CachedB50Entry {
        member: member.clone(),
        player: CachedPlayer {
            nickname: Some("player".to_owned()),
            rating: Some(15000),
            ..CachedPlayer::default()
        },
        rating_breakdown: RatingBreakdown {
            b35: 10000,
            b15: 5000,
            total: 15000,
        },
        fit_index: super::CachedFitIndex {
            label: Some(super::CachedFitIndexLabel::SlightlyInflated),
            b50: super::CachedFitIndexSection {
                virtual_rating: Some(38),
                virtual_ratio_percent: Some(super::CachedExactRatio {
                    numerator: 1,
                    denominator: 4,
                }),
                counted: 1,
                ..super::CachedFitIndexSection::default()
            },
            ..super::CachedFitIndex::default()
        },
        charts: vec![CachedB50Chart {
            section: B50Section::B35,
            ordinal: 0,
            chart: chart(383)?,
        }],
    };
    store
        .complete_b50_ranking(
            &snapshot(RankingNamespace::B50, &group, b50_job.generation, now),
            &[entry],
            now,
        )
        .await?;

    let song_job = store
        .start_ranking_job(
            RankingNamespace::SongScore,
            &group,
            RankingRefreshReason::Miss,
            now,
        )
        .await?;
    let RankingJobStart::Started(song_job) = song_job else {
        return Err("expected song job start".into());
    };
    store
        .complete_song_ranking(
            &snapshot(
                RankingNamespace::SongScore,
                &group,
                song_job.generation,
                now,
            ),
            std::slice::from_ref(&member),
            &[(member.qq.clone(), chart(383)?)],
            now,
        )
        .await?;

    assert!(
        store
            .clear_ranking_cache(RankingNamespace::B50, &group)
            .await?
    );
    assert!(
        store
            .ranking_cache(RankingNamespace::B50, &group)
            .await?
            .is_none()
    );
    let song = store
        .ranking_cache(RankingNamespace::SongScore, &group)
        .await?
        .ok_or("missing song cache")?;
    assert!(matches!(song.data, RankingSnapshotData::SongScores { .. }));
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&store.pool)
        .await?;
    assert!(violations.is_empty());
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn restart_marks_only_running_jobs_interrupted() -> Result<(), Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    let path = temp.path().join("state.db");
    let store = StateStore::open(&path).await?;
    let group = GroupId::new("restart")?;
    let now = OffsetDateTime::now_utc();
    store
        .start_ranking_job(
            RankingNamespace::B50,
            &group,
            RankingRefreshReason::Miss,
            now,
        )
        .await?;
    store.close().await;

    let reopened = StateStore::open(&path).await?;
    assert_eq!(reopened.interrupt_running_ranking_jobs(now).await?, 1);
    let job = reopened
        .ranking_job(RankingNamespace::B50, &group)
        .await?
        .ok_or("missing job")?;
    assert_eq!(job.status, RankingJobStatus::Interrupted);
    reopened.close().await;
    Ok(())
}

fn snapshot(
    namespace: RankingNamespace,
    group: &GroupId,
    generation: u64,
    now: OffsetDateTime,
) -> RankingSnapshot {
    RankingSnapshot {
        namespace,
        group_id: group.clone(),
        generation,
        fetched_at: now,
        next_reset_at: now + Duration::days(1),
        member_count: 1,
        success_count: 1,
        failure_count: 0,
        skipped_count: 0,
        cache_hit_count: 0,
        shared_fetch_count: 0,
    }
}

fn member(ordinal: u32, qq: &str) -> Result<RankingMember, Box<dyn Error + Send + Sync>> {
    Ok(RankingMember {
        ordinal,
        qq: QqId::new(qq)?,
        nickname: Some("qq-name".to_owned()),
        card: Some("group-card".to_owned()),
        display_name: "group-card".to_owned(),
        waterfish_nickname: Some("player".to_owned()),
        waterfish_username: None,
    })
}

fn chart(song_id: u32) -> Result<CachedChart, Box<dyn Error + Send + Sync>> {
    Ok(CachedChart {
        key: ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::DivingFish, song_id),
            ChartGeneration::Standard,
            Difficulty::Master,
        )?,
        title: "stable title".to_owned(),
        level: "13".to_owned(),
        constant: None,
        achievements: Some(AchievementRate::from_decimal_str("100.0000")?),
        dx_score: Some(1_000_000),
        rating: Some(300),
        original_rating: None,
        grade: Some("sss".to_owned()),
        full_combo: None,
        full_sync: None,
        version: "version".to_owned(),
        is_current: false,
        fit_constant: None,
    })
}
