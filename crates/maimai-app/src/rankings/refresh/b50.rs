use std::sync::Arc;

use maimai_core::{GroupId, ScoreSource};
use maimai_providers::NapCatClient;
use maimai_storage::{
    B50Section, CachedB50Chart, CachedB50Entry, CachedChart, CachedExactRatio, CachedFitIndex,
    CachedFitIndexLabel, CachedFitIndexSection, CachedPlayer, RankingJobProgress, RankingNamespace,
    RankingSnapshot, WaterfishIdentityProfile,
};
use time::OffsetDateTime;
use tokio::task::JoinSet;

use crate::{
    score_service::{B50Mode, B50Request, ScoreQuery},
    scores::{B50Chart, B50Result, Lookup},
};

use super::super::{
    RankingError, RankingService, RefreshOptions,
    cache::{latest_reset, next_reset},
};

impl RankingService {
    pub(super) async fn refresh_b50(
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
            RankingNamespace::B50,
            group_id,
            generation,
            RankingJobProgress {
                total_count: Some(count(&members)?),
                ..RankingJobProgress::default()
            },
            "已读取群成员，正在并发查询 B50。",
        )
        .await?;

        let fresh_after = latest_reset(started_at);
        let mut entries = Vec::new();
        let mut cache_hits = 0_u32;
        let mut network_members = Vec::new();
        for member in members.iter().cloned() {
            match self
                .scores
                .cached_diving_fish_b50(&member.qq, fresh_after)
                .await
            {
                Ok(Some(result)) => {
                    entries.push(self.accept_b50(member, result, started_at).await?);
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
            spawn_b50(&mut tasks, Arc::clone(&self.scores), member, started_at);
        }
        while let Some(outcome) = tasks.join_next().await {
            let (member, result) = outcome.map_err(|_| RankingError::Task)?;
            processed = processed.checked_add(1).ok_or(RankingError::Task)?;
            match result {
                Ok(result) => {
                    entries.push(self.accept_b50(member, result, started_at).await?);
                }
                Err(error) if is_skippable(&error) => {
                    skipped = skipped.checked_add(1).ok_or(RankingError::Task)?;
                }
                Err(error) => return Err(RankingError::Scores(error)),
            }
            self.progress(
                RankingNamespace::B50,
                group_id,
                generation,
                RankingJobProgress {
                    processed_count: processed,
                    total_count: Some(member_count),
                    cached_count: count(&entries)?,
                    skipped_count: skipped,
                    transient_failure_count: 0,
                    current_qq: None,
                },
                "正在整理群成员 B50。",
            )
            .await?;
            if let Some(member) = pending.next() {
                if !options.query_delay.get().is_zero() {
                    tokio::time::sleep(options.query_delay.get()).await;
                }
                spawn_b50(&mut tasks, Arc::clone(&self.scores), member, started_at);
            }
        }
        entries.sort_by_key(|entry| entry.member.ordinal);
        let finished_at = self.now();
        let snapshot = RankingSnapshot {
            namespace: RankingNamespace::B50,
            group_id: group_id.clone(),
            generation,
            fetched_at: finished_at,
            next_reset_at: next_reset(finished_at),
            member_count,
            success_count: count(&entries)?,
            failure_count: 0,
            skipped_count: skipped,
            cache_hit_count: cache_hits,
            shared_fetch_count: 0,
        };
        self.store
            .complete_b50_ranking(&snapshot, &entries, finished_at)
            .await?;
        Ok(())
    }

    async fn accept_b50(
        &self,
        member: maimai_storage::RankingMember,
        result: B50Result,
        updated_at: OffsetDateTime,
    ) -> Result<CachedB50Entry, RankingError> {
        let profile = WaterfishIdentityProfile {
            nickname: result.player.nickname.clone(),
            username: result
                .player
                .username
                .as_ref()
                .map(maimai_core::PlayerUsername::new)
                .transpose()
                .map_err(|_| RankingError::Task)?,
            rating: result.player.rating,
        };
        self.identity
            .upsert_waterfish_identity(&member.qq, &profile, updated_at)
            .await?;
        cached_entry(member, result)
    }

    pub(super) async fn progress(
        &self,
        namespace: RankingNamespace,
        group_id: &GroupId,
        generation: u64,
        progress: RankingJobProgress,
        message: &str,
    ) -> Result<(), RankingError> {
        if self
            .store
            .update_ranking_job_progress(namespace, group_id, generation, message, &progress)
            .await?
        {
            Ok(())
        } else {
            Err(RankingError::Task)
        }
    }
}

fn spawn_b50(
    tasks: &mut JoinSet<(
        maimai_storage::RankingMember,
        Result<B50Result, crate::score_service::PlayerScoreServiceError>,
    )>,
    scores: Arc<crate::score_service::PlayerScoreService>,
    member: maimai_storage::RankingMember,
    started_at: OffsetDateTime,
) {
    tasks.spawn(async move {
        let mut query = ScoreQuery::new(Lookup::Qq(member.qq.clone()), started_at.unix_timestamp());
        query.source = Some(ScoreSource::DivingFish);
        let result = scores
            .b50(B50Request {
                query,
                mode: B50Mode::Provider,
            })
            .await;
        (member, result)
    });
}

fn cached_entry(
    member: maimai_storage::RankingMember,
    result: B50Result,
) -> Result<CachedB50Entry, RankingError> {
    let fit_index = cached_fit_index(&result)?;
    let charts = result
        .b35
        .iter()
        .enumerate()
        .map(|(ordinal, chart)| cached_b50_chart(B50Section::B35, ordinal, chart))
        .chain(
            result
                .b15
                .iter()
                .enumerate()
                .map(|(ordinal, chart)| cached_b50_chart(B50Section::B15, ordinal, chart)),
        )
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CachedB50Entry {
        member,
        player: CachedPlayer {
            nickname: result.player.nickname,
            username: result.player.username,
            rating: result.player.rating,
            actual_rating: result.player.actual_rating,
            additional_rating: result.player.additional_rating,
            plate: result.player.plate,
        },
        rating_breakdown: result.rating_breakdown,
        fit_index,
        charts,
    })
}

pub(super) fn cached_chart(chart: &B50Chart) -> CachedChart {
    CachedChart {
        key: chart.key.clone(),
        title: chart.title.clone(),
        level: chart.level.clone(),
        constant: chart.constant,
        achievements: chart.achievements.and_then(|value| value.ranked()),
        dx_score: chart.dx_score,
        rating: chart.rating,
        original_rating: chart.original_rating,
        grade: chart.grade.clone(),
        full_combo: chart.full_combo,
        full_sync: chart.full_sync,
        version: chart.version.clone(),
        is_current: chart.is_current,
        fit_constant: chart.fit_constant,
    }
}

fn cached_b50_chart(
    section: B50Section,
    ordinal: usize,
    chart: &B50Chart,
) -> Result<CachedB50Chart, RankingError> {
    Ok(CachedB50Chart {
        section,
        ordinal: u32::try_from(ordinal).map_err(|_| RankingError::Task)?,
        chart: cached_chart(chart),
    })
}

fn cached_fit_index(result: &B50Result) -> Result<CachedFitIndex, RankingError> {
    Ok(CachedFitIndex {
        label: result.fit_index.label.map(|label| match label {
            crate::scores::FitIndexLabel::ClearlyInflated => CachedFitIndexLabel::ClearlyInflated,
            crate::scores::FitIndexLabel::SlightlyInflated => CachedFitIndexLabel::SlightlyInflated,
            crate::scores::FitIndexLabel::Balanced => CachedFitIndexLabel::Balanced,
            crate::scores::FitIndexLabel::SlightlyDeflated => CachedFitIndexLabel::SlightlyDeflated,
            crate::scores::FitIndexLabel::ClearlyDeflated => CachedFitIndexLabel::ClearlyDeflated,
        }),
        b50: cached_fit_section(result.fit_index.b50)?,
        b35: cached_fit_section(result.fit_index.b35)?,
        b15: cached_fit_section(result.fit_index.b15)?,
    })
}

fn cached_fit_section(
    value: crate::scores::FitIndexSection,
) -> Result<CachedFitIndexSection, RankingError> {
    Ok(CachedFitIndexSection {
        virtual_rating: value.virtual_rating,
        virtual_ratio_percent: value.virtual_ratio_percent.map(|ratio| CachedExactRatio {
            numerator: ratio.numerator(),
            denominator: ratio.denominator(),
        }),
        weighted_average_delta: value.weighted_average_delta.map(|ratio| CachedExactRatio {
            numerator: ratio.numerator(),
            denominator: ratio.denominator(),
        }),
        counted: u32::try_from(value.counted).map_err(|_| RankingError::Task)?,
        missing: u32::try_from(value.missing).map_err(|_| RankingError::Task)?,
        total_rating: value.total_rating,
    })
}

fn is_skippable(error: &crate::score_service::PlayerScoreServiceError) -> bool {
    use crate::score_service::PlayerScoreServiceErrorCode::{AuthRequired, SourceUnavailable};
    matches!(error.code(), SourceUnavailable | AuthRequired)
        || matches!(error.status(), Some(400 | 403 | 404))
}

fn count<T>(values: &[T]) -> Result<u32, RankingError> {
    u32::try_from(values.len()).map_err(|_| RankingError::Task)
}
