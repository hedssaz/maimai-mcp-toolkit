use maimai_core::GroupId;
use maimai_storage::{RankingJob, RankingNamespace, RankingSnapshotData};
use time::OffsetDateTime;

use crate::daily_reset::{self, DailyResetHour};

use super::{CacheStatus, RankingError, RankingService};

impl RankingService {
    pub async fn cache_status(
        &self,
        namespace: RankingNamespace,
        group_id: &GroupId,
        now: OffsetDateTime,
    ) -> Result<CacheStatus, RankingError> {
        self.initialize(now).await?;
        self.cache_status_unchecked(namespace, group_id, now).await
    }

    pub async fn job_status(
        &self,
        namespace: RankingNamespace,
        group_id: &GroupId,
        now: OffsetDateTime,
    ) -> Result<Option<RankingJob>, RankingError> {
        self.initialize(now).await?;
        let key = (namespace, group_id.as_str().to_owned());
        if self.state.lock().await.terminal_errors.contains_key(&key) {
            return Err(RankingError::Task);
        }
        Ok(self.store.ranking_job(namespace, group_id).await?)
    }

    pub async fn clear_cache(
        &self,
        namespace: RankingNamespace,
        group_id: &GroupId,
    ) -> Result<bool, RankingError> {
        Ok(self.store.clear_ranking_cache(namespace, group_id).await?)
    }

    pub(crate) async fn cache_status_unchecked(
        &self,
        namespace: RankingNamespace,
        group_id: &GroupId,
        now: OffsetDateTime,
    ) -> Result<CacheStatus, RankingError> {
        let cache = self.store.ranking_cache(namespace, group_id).await?;
        let snapshot = cache.as_ref().map(|cache| &cache.snapshot);
        Ok(CacheStatus {
            namespace,
            group_id: group_id.clone(),
            exists: snapshot.is_some(),
            fresh: snapshot.is_some_and(|snapshot| snapshot.fetched_at >= latest_reset(now)),
            age_seconds: snapshot.map(|snapshot| (now - snapshot.fetched_at).whole_seconds()),
            fetched_at: snapshot.map(|snapshot| snapshot.fetched_at),
            next_reset_at: next_reset(now),
            member_count: snapshot.map(|snapshot| snapshot.member_count),
            success_count: snapshot.map(|snapshot| snapshot.success_count),
            failure_count: snapshot.map(|snapshot| snapshot.failure_count),
            skipped_count: snapshot.map(|snapshot| snapshot.skipped_count),
            cache_hit_count: snapshot.map(|snapshot| snapshot.cache_hit_count),
            shared_fetch_count: snapshot.map(|snapshot| snapshot.shared_fetch_count),
            job: self.store.ranking_job(namespace, group_id).await?,
        })
    }

    pub(crate) async fn b50_cache(
        &self,
        group_id: &GroupId,
    ) -> Result<Option<Vec<maimai_storage::CachedB50Entry>>, RankingError> {
        let cache = self
            .store
            .ranking_cache(RankingNamespace::B50, group_id)
            .await?;
        match cache.map(|cache| cache.data) {
            Some(RankingSnapshotData::B50(entries)) => Ok(Some(entries)),
            Some(RankingSnapshotData::SongScores { .. }) => Err(RankingError::Task),
            None => Ok(None),
        }
    }

    pub(crate) async fn song_cache(
        &self,
        group_id: &GroupId,
    ) -> Result<
        Option<(
            Vec<maimai_storage::RankingMember>,
            Vec<(maimai_core::QqId, maimai_storage::CachedChart)>,
        )>,
        RankingError,
    > {
        let cache = self
            .store
            .ranking_cache(RankingNamespace::SongScore, group_id)
            .await?;
        match cache.map(|cache| cache.data) {
            Some(RankingSnapshotData::SongScores { members, records }) => {
                Ok(Some((members, records)))
            }
            Some(RankingSnapshotData::B50(_)) => Err(RankingError::Task),
            None => Ok(None),
        }
    }
}

pub(crate) fn latest_reset(now: OffsetDateTime) -> OffsetDateTime {
    daily_reset::latest(now, DailyResetHour::UTC_14)
}

pub(crate) fn next_reset(now: OffsetDateTime) -> OffsetDateTime {
    daily_reset::next(now, DailyResetHour::UTC_14)
}
