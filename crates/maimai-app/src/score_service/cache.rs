use maimai_core::{QqId, ScoreSource};
use maimai_storage::{
    B50Section, CachedB50Chart, CachedChart, CachedExactRatio, CachedFitIndex, CachedFitIndexLabel,
    CachedFitIndexSection, CachedPlayer, PlayerB50Snapshot,
};
use time::OffsetDateTime;

use crate::daily_reset::{self, DailyResetHour};
use crate::scores::{
    B50Chart, B50Result, ExactRatio, FitIndex, FitIndexLabel, FitIndexSection, FitLabel, Lookup,
    PlayerScoreProfile, PlayerScores, RatingMode, from_stored_records,
};

use super::{PlayerScoreService, PlayerScoreServiceError};

impl PlayerScoreService {
    pub async fn cached_diving_fish_b50(
        &self,
        qq: &QqId,
        fresh_after: OffsetDateTime,
    ) -> Result<Option<B50Result>, PlayerScoreServiceError> {
        let snapshot = self
            .store
            .player_b50_snapshot(qq, fresh_after)
            .await
            .map_err(|_| PlayerScoreServiceError::storage())?;
        snapshot
            .filter(|snapshot| snapshot.fit_index().available())
            .map(from_cached_snapshot)
            .transpose()
    }

    pub async fn cached_full_scores(
        &self,
        qq: &QqId,
        source: ScoreSource,
        fresh_after: OffsetDateTime,
    ) -> Result<Option<PlayerScores>, PlayerScoreServiceError> {
        let snapshot = self
            .store
            .full_score_snapshot(qq, source, fresh_after)
            .await
            .map_err(|_| PlayerScoreServiceError::storage())?;
        snapshot
            .map(|snapshot| {
                let (profile, records) = snapshot.into_parts();
                let catalog = self.catalog.snapshot();
                from_stored_records(
                    Lookup::Qq(qq.clone()),
                    &records,
                    Some(&profile),
                    &catalog,
                    source,
                )
                .map_err(PlayerScoreServiceError::score)
            })
            .transpose()
    }

    pub(super) async fn cache_diving_fish_b50_best_effort(&self, result: &B50Result, now: i64) {
        let Ok(fetched_at) = OffsetDateTime::from_unix_timestamp(now) else {
            return;
        };
        let Ok(snapshot) = cached_snapshot(result, fetched_at) else {
            return;
        };
        let _outcome = self
            .store
            .replace_player_b50_snapshot(
                &snapshot,
                daily_reset::latest(fetched_at, DailyResetHour::UTC_14),
            )
            .await;
    }
}

fn from_cached_snapshot(snapshot: PlayerB50Snapshot) -> Result<B50Result, PlayerScoreServiceError> {
    let mut b35 = Vec::new();
    let mut b15 = Vec::new();
    for cached in snapshot.charts() {
        let chart =
            B50Chart {
                key: cached.chart.key.clone(),
                source_song_id: cached.chart.key.song().clone(),
                title: cached.chart.title.clone(),
                level: cached.chart.level.clone(),
                constant: cached.chart.constant,
                achievements: cached.chart.achievements.map(Into::into),
                dx_score: cached.chart.dx_score,
                rating: cached.chart.rating,
                original_rating: cached.chart.original_rating,
                grade: cached.chart.grade.clone(),
                full_combo: cached.chart.full_combo,
                full_sync: cached.chart.full_sync,
                version: cached.chart.version.clone(),
                is_current: cached.chart.is_current,
                fit_constant: cached.chart.fit_constant,
                fit_label: cached.chart.constant.zip(cached.chart.fit_constant).map(
                    |(actual, fit)| match actual.cmp(&fit) {
                        std::cmp::Ordering::Greater => FitLabel::Inflated,
                        std::cmp::Ordering::Less => FitLabel::Deflated,
                        std::cmp::Ordering::Equal => FitLabel::Equal,
                    },
                ),
            };
        match cached.section {
            B50Section::B35 => b35.push((cached.ordinal, chart)),
            B50Section::B15 => b15.push((cached.ordinal, chart)),
        }
    }
    b35.sort_by_key(|(ordinal, _)| *ordinal);
    b15.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(B50Result {
        lookup: Lookup::Qq(snapshot.qq().clone()),
        source: snapshot.source(),
        player: PlayerScoreProfile {
            nickname: snapshot.player().nickname.clone(),
            username: snapshot.player().username.clone(),
            rating: snapshot.player().rating,
            actual_rating: snapshot.player().actual_rating,
            additional_rating: snapshot.player().additional_rating,
            plate: snapshot.player().plate.clone(),
        },
        rating_breakdown: snapshot.rating_breakdown(),
        b35: b35.into_iter().map(|(_, chart)| chart).collect(),
        b15: b15.into_iter().map(|(_, chart)| chart).collect(),
        mode: RatingMode::Actual,
        computation: None,
        fit_index: fit_index(snapshot.fit_index())?,
    })
}

fn fit_index(value: CachedFitIndex) -> Result<FitIndex, PlayerScoreServiceError> {
    Ok(FitIndex {
        label: value.label.map(|label| match label {
            CachedFitIndexLabel::ClearlyInflated => FitIndexLabel::ClearlyInflated,
            CachedFitIndexLabel::SlightlyInflated => FitIndexLabel::SlightlyInflated,
            CachedFitIndexLabel::Balanced => FitIndexLabel::Balanced,
            CachedFitIndexLabel::SlightlyDeflated => FitIndexLabel::SlightlyDeflated,
            CachedFitIndexLabel::ClearlyDeflated => FitIndexLabel::ClearlyDeflated,
        }),
        b50: fit_section(value.b50)?,
        b35: fit_section(value.b35)?,
        b15: fit_section(value.b15)?,
    })
}

fn fit_section(value: CachedFitIndexSection) -> Result<FitIndexSection, PlayerScoreServiceError> {
    Ok(FitIndexSection {
        virtual_rating: value.virtual_rating,
        virtual_ratio_percent: value.virtual_ratio_percent.map(exact_ratio).transpose()?,
        weighted_average_delta: value.weighted_average_delta.map(exact_ratio).transpose()?,
        counted: usize::try_from(value.counted).map_err(|_| invalid_cache())?,
        missing: usize::try_from(value.missing).map_err(|_| invalid_cache())?,
        total_rating: value.total_rating,
    })
}

fn exact_ratio(value: CachedExactRatio) -> Result<ExactRatio, PlayerScoreServiceError> {
    ExactRatio::new(value.numerator, value.denominator).ok_or_else(invalid_cache)
}

fn invalid_cache() -> PlayerScoreServiceError {
    PlayerScoreServiceError::new(
        super::PlayerScoreServiceErrorCode::Storage,
        "共享成绩缓存内容无效",
    )
}

fn cached_snapshot(
    result: &B50Result,
    fetched_at: OffsetDateTime,
) -> Result<PlayerB50Snapshot, maimai_storage::StorageError> {
    let Lookup::Qq(qq) = &result.lookup else {
        return Err(invalid("player_b50_cache.lookup", "username"));
    };
    if result.source != ScoreSource::DivingFish || result.mode != RatingMode::Actual {
        return Err(invalid("player_b50_cache.source", "not_df_actual"));
    }
    let mut charts = Vec::with_capacity(result.total_count());
    charts.extend(cached_charts(B50Section::B35, &result.b35)?);
    charts.extend(cached_charts(B50Section::B15, &result.b15)?);
    PlayerB50Snapshot::new(
        qq.clone(),
        ScoreSource::DivingFish,
        fetched_at,
        CachedPlayer {
            nickname: result.player.nickname.clone(),
            username: result.player.username.clone(),
            rating: result.player.rating,
            actual_rating: result.player.actual_rating,
            additional_rating: result.player.additional_rating,
            plate: result.player.plate.clone(),
        },
        result.rating_breakdown,
        cached_fit_index(result)?,
        charts,
    )
}

fn cached_charts(
    section: B50Section,
    charts: &[B50Chart],
) -> Result<Vec<CachedB50Chart>, maimai_storage::StorageError> {
    charts
        .iter()
        .enumerate()
        .map(|(ordinal, chart)| {
            Ok(CachedB50Chart {
                section,
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| invalid("player_b50_cache.ordinal", "overflow"))?,
                chart: CachedChart {
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
                },
            })
        })
        .collect()
}

fn cached_fit_index(result: &B50Result) -> Result<CachedFitIndex, maimai_storage::StorageError> {
    Ok(CachedFitIndex {
        label: result.fit_index.label.map(|label| match label {
            FitIndexLabel::ClearlyInflated => CachedFitIndexLabel::ClearlyInflated,
            FitIndexLabel::SlightlyInflated => CachedFitIndexLabel::SlightlyInflated,
            FitIndexLabel::Balanced => CachedFitIndexLabel::Balanced,
            FitIndexLabel::SlightlyDeflated => CachedFitIndexLabel::SlightlyDeflated,
            FitIndexLabel::ClearlyDeflated => CachedFitIndexLabel::ClearlyDeflated,
        }),
        b50: cached_fit_section(result.fit_index.b50)?,
        b35: cached_fit_section(result.fit_index.b35)?,
        b15: cached_fit_section(result.fit_index.b15)?,
    })
}

fn cached_fit_section(
    value: FitIndexSection,
) -> Result<CachedFitIndexSection, maimai_storage::StorageError> {
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
        counted: u32::try_from(value.counted)
            .map_err(|_| invalid("player_b50_cache.fit_counted", "overflow"))?,
        missing: u32::try_from(value.missing)
            .map_err(|_| invalid("player_b50_cache.fit_missing", "overflow"))?,
        total_rating: value.total_rating,
    })
}

fn invalid(field: &'static str, value: &str) -> maimai_storage::StorageError {
    maimai_storage::StorageError::InvalidStoredValue {
        field,
        value: value.to_owned(),
    }
}
