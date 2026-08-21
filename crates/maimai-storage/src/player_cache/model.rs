use std::collections::HashSet;

use maimai_core::{QqId, RatingBreakdown, ScoreSource};
use time::OffsetDateTime;

use crate::{CachedB50Chart, CachedFitIndex, CachedPlayer, StorageError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B50CacheWriteOutcome {
    Written,
    PreservedHigherQuality,
    Unchanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerB50Snapshot {
    qq: QqId,
    source: ScoreSource,
    fetched_at: OffsetDateTime,
    player: CachedPlayer,
    rating_breakdown: RatingBreakdown,
    fit_index: CachedFitIndex,
    charts: Vec<CachedB50Chart>,
}

impl PlayerB50Snapshot {
    pub fn new(
        qq: QqId,
        source: ScoreSource,
        fetched_at: OffsetDateTime,
        player: CachedPlayer,
        rating_breakdown: RatingBreakdown,
        fit_index: CachedFitIndex,
        charts: Vec<CachedB50Chart>,
    ) -> Result<Self, StorageError> {
        let snapshot = Self {
            qq,
            source,
            fetched_at,
            player,
            rating_breakdown,
            fit_index,
            charts,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn qq(&self) -> &QqId {
        &self.qq
    }

    pub const fn source(&self) -> ScoreSource {
        self.source
    }

    pub const fn fetched_at(&self) -> OffsetDateTime {
        self.fetched_at
    }

    pub const fn player(&self) -> &CachedPlayer {
        &self.player
    }

    pub const fn rating_breakdown(&self) -> RatingBreakdown {
        self.rating_breakdown
    }

    pub const fn fit_index(&self) -> CachedFitIndex {
        self.fit_index
    }

    pub fn charts(&self) -> &[CachedB50Chart] {
        &self.charts
    }

    pub(super) fn quality(&self) -> i64 {
        if self.fit_index.available() {
            2
        } else if self.charts.is_empty() {
            0
        } else {
            1
        }
    }

    fn validate(&self) -> Result<(), StorageError> {
        if self.source != ScoreSource::DivingFish {
            return Err(invalid("player_b50_cache.source", "not_diving_fish"));
        }
        for section in [self.fit_index.b50, self.fit_index.b35, self.fit_index.b15] {
            for ratio in [
                section.virtual_ratio_percent,
                section.weighted_average_delta,
            ]
            .into_iter()
            .flatten()
            {
                if ratio.denominator == 0 {
                    return Err(invalid("player_b50_cache.fit_ratio", "zero_denominator"));
                }
            }
        }
        let mut keys = HashSet::new();
        let mut expected_b35 = 0u32;
        let mut expected_b15 = 0u32;
        for cached in &self.charts {
            if !keys.insert(cached.chart.key.clone()) {
                return Err(invalid("player_b50_cache.chart", "duplicate"));
            }
            let expected = match cached.section {
                crate::B50Section::B35 => &mut expected_b35,
                crate::B50Section::B15 => &mut expected_b15,
            };
            if cached.ordinal != *expected {
                return Err(invalid("player_b50_cache.ordinal", "non_contiguous"));
            }
            *expected += 1;
        }
        if expected_b35 > 35 || expected_b15 > 15 {
            return Err(invalid("player_b50_cache.charts", "section_too_large"));
        }
        Ok(())
    }
}

fn invalid(field: &'static str, value: &str) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.to_owned(),
    }
}
