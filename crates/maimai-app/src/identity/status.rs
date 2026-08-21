use maimai_storage::IdentityStats;
use time::{Duration, OffsetDateTime, PrimitiveDateTime};

use super::{CacheStatus, IdentityDirectory, IdentityError, IdentityService, ResetHour};

pub const DEFAULT_RESET_HOUR_UTC: u8 = 14;

impl IdentityService {
    pub async fn cache_status(
        &self,
        now: OffsetDateTime,
        reset_hour: ResetHour,
    ) -> Result<CacheStatus, IdentityError> {
        self.initialize_identity_jobs(now).await?;
        self.directory.cache_status(now, reset_hour).await
    }

    pub(crate) async fn cache_status_unchecked(
        &self,
        now: OffsetDateTime,
        reset_hour: ResetHour,
    ) -> Result<CacheStatus, IdentityError> {
        self.directory.cache_status(now, reset_hour).await
    }
}

impl IdentityDirectory {
    pub async fn cache_status(
        &self,
        now: OffsetDateTime,
        reset_hour: ResetHour,
    ) -> Result<CacheStatus, IdentityError> {
        let metadata = self.store().identity_metadata().await?;
        let reset = latest_reset(now, reset_hour);
        let fresh = metadata
            .as_ref()
            .and_then(|metadata| metadata.fetched_at)
            .is_some_and(|fetched_at| fetched_at >= reset);
        let age_seconds = metadata
            .as_ref()
            .and_then(|metadata| metadata.fetched_at)
            .map(|fetched_at| (now - fetched_at).whole_seconds());
        Ok(CacheStatus {
            exists: metadata.is_some(),
            fresh,
            age_seconds,
            reset_hour_utc: reset_hour,
            fetched_at: metadata.as_ref().and_then(|metadata| metadata.fetched_at),
            updated_at: metadata.as_ref().map(|metadata| metadata.updated_at),
            generation: metadata.as_ref().map(|metadata| metadata.generation),
            stats: metadata
                .as_ref()
                .map_or_else(IdentityStats::default, |metadata| metadata.stats),
        })
    }
}

pub(super) fn latest_reset(now: OffsetDateTime, reset_hour: ResetHour) -> OffsetDateTime {
    let reset = PrimitiveDateTime::new(now.date(), reset_hour.time()).assume_utc();
    if now < reset {
        reset - Duration::days(1)
    } else {
        reset
    }
}
