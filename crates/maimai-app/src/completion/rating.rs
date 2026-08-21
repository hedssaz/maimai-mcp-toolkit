use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
};

use maimai_catalog::{CatalogQuery, CatalogSnapshot, Region, SourceKind};
use maimai_core::{
    AchievementRate, ChartConstant, Difficulty, FullComboStatus, FullSyncStatus, achievement_rank,
};
use maimai_render::{
    RatingAllClear, RatingConstantGroup, RatingScoreCell, RatingStatistics, RatingTableMode,
    RatingTableView,
};

use crate::scores::{B50Chart, PlayerScores, best_by_chart};

use super::{CompletionError, RatingTableRequest};

pub(crate) fn prepare(
    snapshot: &CatalogSnapshot,
    scores: &PlayerScores,
    request: &RatingTableRequest,
    allow_jp: bool,
) -> Result<RatingTableView, CompletionError> {
    request.validate()?;
    let mut query = CatalogQuery {
        level: Some(request.level.trim().to_owned()),
        ..CatalogQuery::default()
    };
    if !allow_jp {
        query.region_has = BTreeSet::from([Region::China]);
    }
    let records = best_by_chart(&scores.records);
    let mut groups = BTreeMap::<Reverse<Option<ChartConstant>>, Vec<RatingScoreCell>>::new();
    let mut statistics = RatingStatistics::default();
    for hit in snapshot.query(&query)? {
        for matched in &hit.matched_charts {
            if matched.chart.key.difficulty() == Difficulty::Utage {
                continue;
            }
            let record = records.get(&matched.chart.key).copied();
            if let Some(record) = record {
                accumulate(&mut statistics, record);
            }
            groups
                .entry(Reverse(matched.chart.constant))
                .or_default()
                .push(cell(&hit, matched, record));
        }
    }
    let groups = groups
        .into_iter()
        .map(|(Reverse(constant), cells)| RatingConstantGroup { constant, cells })
        .collect::<Vec<_>>();
    let total = groups.iter().map(|group| group.cells.len()).sum();
    Ok(RatingTableView {
        level: request.level.trim().to_owned(),
        mode: request.mode,
        total,
        statistics,
        all_clear: all_clear(request.mode, total, &groups),
        groups,
    })
}

fn cell(
    hit: &maimai_catalog::SearchHit<'_>,
    matched: &maimai_catalog::MatchedChart<'_>,
    record: Option<&B50Chart>,
) -> RatingScoreCell {
    let source = matched
        .source_matches
        .iter()
        .find(|source| source.song.source == SourceKind::DivingFish)
        .or_else(|| matched.source_matches.first());
    RatingScoreCell {
        cover_id: source.map_or_else(
            || hit.music.primary_id.value().clone(),
            |source| source.song.id.value().clone(),
        ),
        image_name: source.and_then(|source| source.song.image_name.clone()),
        title: hit.music.title.clone(),
        generation: matched.chart.key.generation(),
        difficulty: matched.chart.key.difficulty(),
        achievement: record.and_then(|record| {
            record
                .achievements
                .and_then(|achievement| achievement.ranked())
        }),
        combo: record.and_then(|record| record.full_combo),
    }
}

fn accumulate(statistics: &mut RatingStatistics, record: &B50Chart) {
    if let Some(achievement) = record.achievements.and_then(|value| value.ranked()) {
        let value = achievement.ten_thousandths();
        statistics.clear += usize::from(value >= 800_000);
        statistics.s += usize::from(value >= 970_000);
        statistics.sp += usize::from(value >= 980_000);
        statistics.ss += usize::from(value >= 990_000);
        statistics.ssp += usize::from(value >= 995_000);
        statistics.sss += usize::from(value >= 1_000_000);
        statistics.sssp += usize::from(value >= 1_005_000);
    }
    if let Some(combo) = record.full_combo {
        statistics.fc += 1;
        statistics.fcp += usize::from(combo >= FullComboStatus::FullComboPlus);
        statistics.ap += usize::from(combo >= FullComboStatus::AllPerfect);
        statistics.app += usize::from(combo >= FullComboStatus::AllPerfectPlus);
    }
    if let Some(sync) = record.full_sync {
        match sync {
            FullSyncStatus::Sync => statistics.sync += 1,
            FullSyncStatus::FullSync => statistics.fs += 1,
            FullSyncStatus::FullSyncPlus => {
                statistics.fs += 1;
                statistics.fsp += 1;
            }
            FullSyncStatus::FullSyncDeluxe => {
                statistics.fs += 1;
                statistics.fsp += 1;
                statistics.fsd += 1;
            }
            FullSyncStatus::FullSyncDeluxePlus => {
                statistics.fs += 1;
                statistics.fsp += 1;
                statistics.fsd += 1;
                statistics.fsdp += 1;
            }
        }
    }
}

fn all_clear(
    mode: RatingTableMode,
    total: usize,
    groups: &[RatingConstantGroup],
) -> Option<RatingAllClear> {
    if total == 0 {
        return None;
    }
    match mode {
        RatingTableMode::Achievement => {
            let mut minimum = None::<AchievementRate>;
            for cell in groups.iter().flat_map(|group| &group.cells) {
                let value = cell.achievement?;
                minimum = Some(minimum.map_or(value, |current| current.min(value)));
            }
            minimum
                .filter(|value| value.ten_thousandths() >= 970_000)
                .map(|value| RatingAllClear::Achievement(achievement_rank(value)))
        }
        RatingTableMode::FullCombo => {
            let mut minimum = None::<FullComboStatus>;
            for cell in groups.iter().flat_map(|group| &group.cells) {
                let value = cell.combo?;
                minimum = Some(minimum.map_or(value, |current| current.min(value)));
            }
            minimum.map(RatingAllClear::FullCombo)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{accumulate, all_clear, prepare};
    use crate::scores::{B50Chart, Lookup, PlayerScoreProfile, PlayerScores};
    use maimai_catalog::{CatalogFiles, CatalogQuery, SourceKind};
    use maimai_core::{
        AchievementRank, AchievementRate, ChartGeneration, Difficulty, FullComboStatus,
        FullSyncStatus, PlayerUsername, ScoreSource, SongIdValue,
    };
    use maimai_render::{
        RatingAllClear, RatingConstantGroup, RatingScoreCell, RatingStatistics, RatingTableMode,
    };

    use crate::completion::{CompletionIdentity, RatingTableRequest};

    fn cell(
        achievement: Option<&str>,
        combo: Option<FullComboStatus>,
    ) -> Result<RatingScoreCell, Box<dyn std::error::Error>> {
        Ok(RatingScoreCell {
            cover_id: SongIdValue::Numeric(1),
            image_name: None,
            title: "x".to_owned(),
            generation: ChartGeneration::Standard,
            difficulty: Difficulty::Master,
            achievement: achievement
                .map(AchievementRate::from_decimal_str)
                .transpose()?,
            combo,
        })
    }

    #[test]
    fn all_clear_requires_every_chart_and_uses_the_lowest_marker()
    -> Result<(), Box<dyn std::error::Error>> {
        let groups = vec![RatingConstantGroup {
            constant: None,
            cells: vec![cell(Some("100.5"), None)?, cell(Some("99"), None)?],
        }];
        assert_eq!(
            all_clear(RatingTableMode::Achievement, 2, &groups),
            Some(RatingAllClear::Achievement(AchievementRank::Ss))
        );
        let missing = vec![RatingConstantGroup {
            constant: None,
            cells: vec![cell(Some("100.5"), None)?, cell(None, None)?],
        }];
        assert_eq!(all_clear(RatingTableMode::Achievement, 2, &missing), None);
        let fc = vec![RatingConstantGroup {
            constant: None,
            cells: vec![
                cell(None, Some(FullComboStatus::AllPerfect))?,
                cell(None, Some(FullComboStatus::FullComboPlus))?,
            ],
        }];
        assert_eq!(
            all_clear(RatingTableMode::FullCombo, 2, &fc),
            Some(RatingAllClear::FullCombo(FullComboStatus::FullComboPlus))
        );
        Ok(())
    }

    #[test]
    fn sixteen_statistics_keep_cumulative_marker_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        let key = maimai_core::ChartKey::new(
            maimai_core::SourceSongId::numeric(maimai_core::SongIdNamespace::DivingFish, 1),
            ChartGeneration::Standard,
            Difficulty::Master,
        )?;
        let mut high = record(key.clone(), "100.5")?;
        high.full_combo = Some(FullComboStatus::AllPerfectPlus);
        high.full_sync = Some(FullSyncStatus::FullSyncDeluxePlus);
        let mut low = record(key, "79.9999")?;
        low.full_combo = Some(FullComboStatus::FullCombo);
        low.full_sync = Some(FullSyncStatus::Sync);
        let mut statistics = RatingStatistics::default();
        accumulate(&mut statistics, &high);
        accumulate(&mut statistics, &low);
        assert_eq!(
            statistics.values(),
            [1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1]
        );
        Ok(())
    }

    #[test]
    fn catalog_order_is_constant_descending_and_main_includes_jp_only_charts()
    -> Result<(), Box<dyn std::error::Error>> {
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let snapshot = CatalogFiles::from_data_dir(data).load()?;
        let scores = PlayerScores {
            lookup: Lookup::Username(PlayerUsername::new("rating-fixture")?),
            source: ScoreSource::DivingFish,
            player: PlayerScoreProfile::default(),
            records: Vec::new(),
        };
        let request = RatingTableRequest {
            identity: CompletionIdentity {
                lookup: scores.lookup.clone(),
                source: None,
            },
            level: "15".to_owned(),
            mode: RatingTableMode::Achievement,
        };
        let main = prepare(&snapshot, &scores, &request, true)?;
        let public = prepare(&snapshot, &scores, &request, false)?;
        assert!(
            main.total > public.total,
            "fixture must retain at least one JP-only chart"
        );
        assert!(
            main.groups
                .windows(2)
                .all(|pair| pair[0].constant >= pair[1].constant)
        );
        assert!(
            main.groups
                .iter()
                .flat_map(|group| &group.cells)
                .all(|cell| cell.difficulty != Difficulty::Utage)
        );
        Ok(())
    }

    #[test]
    fn catalog_matrix_uses_best_record_for_each_typed_chart_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let snapshot = CatalogFiles::from_data_dir(data).load()?;
        let mut selected = None;
        for hit in snapshot.query(&CatalogQuery {
            level: Some("15".to_owned()),
            ..CatalogQuery::default()
        })? {
            if let Some(chart) = hit
                .matched_charts
                .iter()
                .find(|chart| chart.chart.key.difficulty() != Difficulty::Utage)
            {
                selected = Some((
                    hit.music.title.clone(),
                    chart.chart.key.clone(),
                    chart.chart.key.generation(),
                    chart.chart.key.difficulty(),
                    chart
                        .source_matches
                        .iter()
                        .find(|source| source.song.source == SourceKind::DivingFish)
                        .or_else(|| chart.source_matches.first())
                        .map_or_else(
                            || hit.music.primary_id.value().clone(),
                            |source| source.song.id.value().clone(),
                        ),
                ));
                break;
            }
        }
        let (title, key, generation, difficulty, cover_id) =
            selected.ok_or("level 15 fixture missing")?;
        let scores = PlayerScores {
            lookup: Lookup::Username(PlayerUsername::new("best-chart-fixture")?),
            source: ScoreSource::DivingFish,
            player: PlayerScoreProfile::default(),
            records: vec![record(key.clone(), "99")?, record(key, "100.5")?],
        };
        let request = RatingTableRequest {
            identity: CompletionIdentity {
                lookup: scores.lookup.clone(),
                source: None,
            },
            level: "15".to_owned(),
            mode: RatingTableMode::Achievement,
        };
        let view = prepare(&snapshot, &scores, &request, true)?;
        let achievement = view
            .groups
            .iter()
            .flat_map(|group| &group.cells)
            .find(|cell| {
                cell.title == title
                    && cell.generation == generation
                    && cell.difficulty == difficulty
                    && cell.cover_id == cover_id
            })
            .and_then(|cell| cell.achievement)
            .ok_or("selected rating cell missing")?;
        assert_eq!(achievement, AchievementRate::from_decimal_str("100.5")?);
        Ok(())
    }

    fn record(
        key: maimai_core::ChartKey,
        achievement: &str,
    ) -> Result<B50Chart, Box<dyn std::error::Error>> {
        Ok(B50Chart {
            source_song_id: key.song().clone(),
            key,
            title: "fixture".to_owned(),
            level: "15".to_owned(),
            constant: None,
            achievements: Some(AchievementRate::from_decimal_str(achievement)?.into()),
            dx_score: None,
            rating: None,
            original_rating: None,
            grade: None,
            full_combo: None,
            full_sync: None,
            version: String::new(),
            is_current: false,
            fit_constant: None,
            fit_label: None,
        })
    }
}
