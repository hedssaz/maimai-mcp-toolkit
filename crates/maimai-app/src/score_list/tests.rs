use std::path::PathBuf;

use maimai_catalog::{CatalogFiles, CatalogSnapshot, SourceKind};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, PlayAchievement, QqId,
    ScoreSource, UtageScore,
};

use crate::scores::{B50Chart, Lookup, PlayerScoreProfile, PlayerScores};

use super::{ScoreListTarget, prepare};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn snapshot() -> Result<CatalogSnapshot, maimai_catalog::CatalogError> {
    CatalogFiles::from_data_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")).load()
}

fn chart_keys(
    snapshot: &CatalogSnapshot,
    count: usize,
) -> Result<Vec<ChartKey>, Box<dyn std::error::Error>> {
    let mut keys = Vec::new();
    for song in snapshot.songs() {
        for chart in &song.charts {
            if matches!(chart.key.difficulty(), Difficulty::Utage)
                || matches!(
                    chart.key.generation(),
                    ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
                )
            {
                continue;
            }
            if snapshot
                .source_chart(&chart.key, SourceKind::DivingFish)?
                .is_some()
            {
                keys.push(chart.key.clone());
                if keys.len() == count {
                    return Ok(keys);
                }
            }
        }
    }
    Err(format!("catalog only supplied {} suitable charts", keys.len()).into())
}

fn scores(keys: &[ChartKey], level: &str) -> Result<PlayerScores, maimai_core::RatingError> {
    Ok(PlayerScores {
        lookup: Lookup::Qq(QqId::new("10001").map_err(|_| {
            maimai_core::RatingError::InvalidDecimal {
                field: "test",
                value: "qq".to_owned(),
            }
        })?),
        source: ScoreSource::DivingFish,
        player: PlayerScoreProfile::default(),
        records: keys
            .iter()
            .enumerate()
            .map(|(index, key)| {
                Ok(B50Chart {
                    key: key.clone(),
                    source_song_id: key.song().clone(),
                    title: format!("provider-title-{index}"),
                    level: level.to_owned(),
                    constant: Some(ChartConstant::from_decimal_str("14.0")?),
                    achievements: Some(
                        AchievementRate::from_ten_thousandths(
                            900_000 + u32::try_from(index % 100).unwrap_or(0),
                        )?
                        .into(),
                    ),
                    dx_score: Some(1_000),
                    rating: Some(200),
                    original_rating: None,
                    grade: Some("aa".to_owned()),
                    full_combo: None,
                    full_sync: None,
                    version: String::new(),
                    is_current: false,
                    fit_constant: None,
                    fit_label: None,
                })
            })
            .collect::<Result<Vec<_>, maimai_core::RatingError>>()?,
    })
}

#[test]
fn exact_filter_sort_and_diving_fish_projection_are_used() -> TestResult {
    let snapshot = snapshot()?;
    let keys = chart_keys(&snapshot, 3)?;
    let mut values = scores(&keys, "14")?;
    values.records[0].achievements = Some(AchievementRate::from_decimal_str("99")?.into());
    values.records[1].achievements = Some(AchievementRate::from_decimal_str("100")?.into());
    values.records[2].achievements = Some(AchievementRate::from_decimal_str("100")?.into());
    let view = prepare::prepare(&snapshot, &values, &ScoreListTarget::level("14")?, 1)?;
    assert_eq!(view.items.len(), 3);
    let expected_first = snapshot
        .source_chart(&keys[1], SourceKind::DivingFish)?
        .ok_or("missing source chart")?;
    let expected_second = snapshot
        .source_chart(&keys[2], SourceKind::DivingFish)?
        .ok_or("missing source chart")?;
    assert_eq!(
        view.items[0].display_id,
        *expected_first.chart.source_song_id.value()
    );
    assert_eq!(
        view.items[1].display_id,
        *expected_second.chart.source_song_id.value()
    );
    assert_eq!(view.items[0].title, expected_first.song.title);
    assert_eq!(
        view.items[0].max_dx_score,
        expected_first
            .chart
            .note_total
            .map(u64::from)
            .or_else(|| expected_first
                .chart
                .notes
                .map(maimai_core::NoteCounts::total))
            .and_then(|value| value.checked_mul(3))
            .and_then(|value| u32::try_from(value).ok())
    );
    assert!(
        prepare::prepare(
            &snapshot,
            &values,
            &ScoreListTarget::constant(ChartConstant::from_decimal_str("14.1")?),
            1,
        )?
        .items
        .is_empty()
    );
    Ok(())
}

#[test]
fn page_boundaries_79_80_81_and_160_have_no_empty_trailing_page() -> TestResult {
    let snapshot = snapshot()?;
    let keys = chart_keys(&snapshot, 160)?;
    for (count, pages, page, expected) in [
        (79, 1, 1, 79),
        (80, 1, 1, 80),
        (81, 2, 2, 1),
        (160, 2, 2, 80),
    ] {
        let values = scores(&keys[..count], "14")?;
        let view = prepare::prepare(&snapshot, &values, &ScoreListTarget::level("14")?, page)?;
        assert_eq!((view.pages, view.items.len()), (pages, expected));
        assert!(
            prepare::prepare(
                &snapshot,
                &values,
                &ScoreListTarget::level("14")?,
                pages + 1,
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn duplicate_chart_chooses_best_once_and_empty_result_is_a_valid_page() -> TestResult {
    let snapshot = snapshot()?;
    let keys = chart_keys(&snapshot, 1)?;
    let mut values = scores(&keys, "14")?;
    let mut better = values.records[0].clone();
    better.achievements = Some(AchievementRate::from_decimal_str("100.5")?.into());
    values.records.push(better);
    let view = prepare::prepare(&snapshot, &values, &ScoreListTarget::level("14")?, 1)?;
    assert_eq!(view.total, 1);
    assert_eq!(
        view.items[0].achievement,
        Some(AchievementRate::from_decimal_str("100.5")?.into())
    );
    let empty = prepare::prepare(&snapshot, &values, &ScoreListTarget::level("15")?, 1)?;
    assert_eq!((empty.total, empty.pages, empty.first), (0, 1, 0));
    assert!(empty.items.is_empty());
    Ok(())
}

#[test]
fn utage_score_list_keeps_exact_achievement_and_is_not_treated_as_unplayed() -> TestResult {
    let snapshot = snapshot()?;
    let mut selected = None;
    'songs: for song in snapshot.songs() {
        for chart in &song.charts {
            if chart.key.difficulty() == Difficulty::Utage
                && snapshot
                    .source_chart(&chart.key, SourceKind::DivingFish)?
                    .is_some()
            {
                selected = Some((chart.key.clone(), chart.level.clone()));
                break 'songs;
            }
        }
    }
    let (key, level) = selected.ok_or("catalog has no Diving-Fish Utage chart")?;
    let achievement = PlayAchievement::from(UtageScore::from_ten_thousandths(1_535_756));
    let values = PlayerScores {
        lookup: Lookup::Qq(QqId::new("10001")?),
        source: ScoreSource::Local,
        player: PlayerScoreProfile::default(),
        records: vec![B50Chart {
            source_song_id: key.song().clone(),
            key,
            title: "utage".to_owned(),
            level: level.clone(),
            constant: None,
            achievements: Some(achievement),
            dx_score: Some(2_295),
            rating: None,
            original_rating: None,
            grade: None,
            full_combo: None,
            full_sync: None,
            version: String::new(),
            is_current: false,
            fit_constant: None,
            fit_label: None,
        }],
    };

    let view = prepare::prepare(&snapshot, &values, &ScoreListTarget::level(level)?, 1)?;
    assert_eq!(view.items.len(), 1);
    assert_eq!(view.items[0].difficulty, Difficulty::Utage);
    assert_eq!(view.items[0].achievement, Some(achievement));
    Ok(())
}
