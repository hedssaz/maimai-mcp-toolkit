use std::{collections::BTreeMap, sync::Arc};

use maimai_core::{GroupId, ScoreSource};
use maimai_providers::NapCatClient;
use maimai_storage::{CachedChart, RankingJobProgress, RankingNamespace, RankingSnapshot};
use time::OffsetDateTime;
use tokio::task::JoinSet;

use crate::{
    score_service::ScoreQuery,
    scores::{B50Chart, Lookup, PlayerScores},
};

use super::super::{
    RankingError, RankingService, RefreshOptions,
    cache::{latest_reset, next_reset},
};
use super::b50::cached_chart;

impl RankingService {
    pub(super) async fn refresh_song(
        &self,
        group_id: &GroupId,
        generation: u64,
        client: &NapCatClient,
        options: RefreshOptions,
        started_at: OffsetDateTime,
    ) -> Result<(), RankingError> {
        let members = self
            .group_members(group_id, client, options, started_at)
            .await?;
        self.progress(
            RankingNamespace::SongScore,
            group_id,
            generation,
            RankingJobProgress {
                total_count: Some(count(&members)?),
                ..RankingJobProgress::default()
            },
            "已读取群成员，正在并发查询完整成绩。",
        )
        .await?;

        let fresh_after = latest_reset(started_at);
        let mut records = Vec::new();
        let mut successful = Vec::new();
        let mut cache_hits = 0_u32;
        let mut network_members = Vec::new();
        for member in members.iter().cloned() {
            match self
                .scores
                .cached_full_scores(&member.qq, ScoreSource::DivingFish, fresh_after)
                .await
            {
                Ok(Some(scores)) => {
                    records.extend(unique_records(&member.qq, &scores));
                    successful.push(member);
                    cache_hits = cache_hits.checked_add(1).ok_or(RankingError::Task)?;
                }
                Ok(None) | Err(_) => network_members.push(member),
            }
        }
        let mut skipped = 0_u32;
        let mut processed = cache_hits;
        let member_count = count(&members)?;
        let mut pending = network_members.into_iter();
        let mut tasks = JoinSet::new();
        for member in pending.by_ref().take(options.max_concurrency.get()) {
            spawn_records(&mut tasks, Arc::clone(&self.scores), member, started_at);
        }
        while let Some(outcome) = tasks.join_next().await {
            let (member, result) = outcome.map_err(|_| RankingError::Task)?;
            processed = processed.checked_add(1).ok_or(RankingError::Task)?;
            match result {
                Ok(scores) => {
                    records.extend(unique_records(&member.qq, &scores));
                    successful.push(member);
                }
                Err(error) if is_skippable(&error) => {
                    skipped = skipped.checked_add(1).ok_or(RankingError::Task)?;
                }
                Err(error) => return Err(RankingError::Scores(error)),
            }
            self.progress(
                RankingNamespace::SongScore,
                group_id,
                generation,
                RankingJobProgress {
                    processed_count: processed,
                    total_count: Some(member_count),
                    cached_count: count(&successful)?,
                    skipped_count: skipped,
                    transient_failure_count: 0,
                    current_qq: None,
                },
                "正在整理群成员完整成绩。",
            )
            .await?;
            if let Some(member) = pending.next() {
                if !options.query_delay.get().is_zero() {
                    tokio::time::sleep(options.query_delay.get()).await;
                }
                spawn_records(&mut tasks, Arc::clone(&self.scores), member, started_at);
            }
        }
        successful.sort_by_key(|member| member.ordinal);
        let finished_at = self.now();
        let snapshot = RankingSnapshot {
            namespace: RankingNamespace::SongScore,
            group_id: group_id.clone(),
            generation,
            fetched_at: finished_at,
            next_reset_at: next_reset(finished_at),
            member_count,
            success_count: count(&successful)?,
            failure_count: 0,
            skipped_count: skipped,
            cache_hit_count: cache_hits,
            shared_fetch_count: 0,
        };
        self.store
            .complete_song_ranking(&snapshot, &successful, &records, finished_at)
            .await?;
        Ok(())
    }
}

fn spawn_records(
    tasks: &mut JoinSet<(
        maimai_storage::RankingMember,
        Result<PlayerScores, crate::score_service::PlayerScoreServiceError>,
    )>,
    scores: Arc<crate::score_service::PlayerScoreService>,
    member: maimai_storage::RankingMember,
    started_at: OffsetDateTime,
) {
    tasks.spawn(async move {
        let mut query = ScoreQuery::new(Lookup::Qq(member.qq.clone()), started_at.unix_timestamp());
        query.source = Some(ScoreSource::DivingFish);
        let result = scores.records(query).await;
        (member, result)
    });
}

fn unique_records(
    qq: &maimai_core::QqId,
    scores: &PlayerScores,
) -> Vec<(maimai_core::QqId, CachedChart)> {
    let mut unique = BTreeMap::<maimai_core::ChartKey, &B50Chart>::new();
    for record in &scores.records {
        match unique.entry(record.key.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(record);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let existing = entry.get();
                if (record.rating, record.achievements) > (existing.rating, existing.achievements) {
                    entry.insert(record);
                }
            }
        }
    }
    unique
        .into_values()
        .map(|record| (qq.clone(), cached_chart(record)))
        .collect()
}

fn is_skippable(error: &crate::score_service::PlayerScoreServiceError) -> bool {
    matches!(error.status(), Some(400 | 403 | 404))
}

fn count<T>(values: &[T]) -> Result<u32, RankingError> {
    u32::try_from(values.len()).map_err(|_| RankingError::Task)
}
