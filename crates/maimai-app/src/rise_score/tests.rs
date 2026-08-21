use std::collections::BTreeMap;

use maimai_catalog::{
    RegionAvailability, SourceChartProjection, SourceKind, SourceSongFields, SourceSongProjection,
};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, SongIdNamespace,
    SourceSongId,
};
use rust_decimal::Decimal;

use super::{
    catalog::is_current_chart,
    prepare::{
        candidate_floor, effective_candidate_floor, ignored_charts, passes_floor,
        replacement_floor, targets,
    },
    rng::FixedRandom,
    selection::{ExpectedPick, rank_weight, select_expected, select_legacy},
};
use crate::scores::{B50Chart, best_by_chart};

#[test]
fn replacement_floor_is_zero_until_each_section_is_full() -> Result<(), Box<dyn std::error::Error>>
{
    let legacy_partial = records(34, 260, 1_000)?;
    let legacy_full = records(35, 261, 2_000)?;
    let current_partial = records(14, 270, 3_000)?;
    let current_full = records(15, 271, 4_000)?;
    assert_eq!(replacement_floor(&legacy_partial, 35), 0);
    assert_eq!(replacement_floor(&legacy_full, 35), 261);
    assert_eq!(replacement_floor(&current_partial, 15), 0);
    assert_eq!(replacement_floor(&current_full, 15), 271);
    assert_eq!(candidate_floor(&current_partial, &legacy_full, 15), 270);
    assert_eq!(candidate_floor(&legacy_full, &current_full, 35), 261);
    let zero_full = records(35, 0, 5_000)?;
    let low_fallback = records(15, 100, 6_000)?;
    assert_eq!(candidate_floor(&zero_full, &low_fallback, 35), 100);
    assert_eq!(candidate_floor(&[], &[], 15), 250);
    assert_eq!(effective_candidate_floor(0, 270), 252);
    assert_eq!(effective_candidate_floor(261, 270), 270);
    Ok(())
}

#[test]
fn canonical_chart_keys_keep_standard_and_deluxe_records_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let song = SourceSongId::numeric(SongIdNamespace::DivingFish, 383);
    let standard = chart(song.clone(), ChartGeneration::Standard, Difficulty::Master)?;
    let deluxe = chart(song, ChartGeneration::Deluxe, Difficulty::Master)?;
    let records = vec![record(standard.clone(), 300)?, record(deluxe.clone(), 301)?];
    let best = best_by_chart(&records);
    assert_eq!(best.len(), 2);
    assert_eq!(
        best.get(&standard).and_then(|value| value.rating),
        Some(300)
    );
    assert_eq!(best.get(&deluxe).and_then(|value| value.rating), Some(301));
    Ok(())
}

#[test]
fn completed_chart_ignore_does_not_hide_other_generations_or_difficulties()
-> Result<(), Box<dyn std::error::Error>> {
    let song = SourceSongId::numeric(SongIdNamespace::DivingFish, 500);
    let completed = chart(song.clone(), ChartGeneration::Standard, Difficulty::Master)?;
    let deluxe = chart(song.clone(), ChartGeneration::Deluxe, Difficulty::Master)?;
    let expert = chart(song, ChartGeneration::Standard, Difficulty::Expert)?;
    let mut completed_record = record(completed.clone(), 300)?;
    completed_record.achievements = Some(AchievementRate::from_decimal_str("100.5")?.into());
    let ignored = ignored_charts(&[completed_record]);
    assert!(ignored.contains(&completed));
    assert!(!ignored.contains(&deluxe));
    assert!(!ignored.contains(&expert));
    Ok(())
}

#[test]
fn current_section_uses_typed_diving_fish_versions_and_chart_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let source = projection("maimai でらっくす PRiSM", ChartGeneration::Standard)?;
    let versions = vec!["maimai でらっくす PRiSM".to_owned()];
    assert!(is_current_chart(
        &versions,
        std::slice::from_ref(&source),
        ChartGeneration::Standard,
        Difficulty::Master,
    ));
    assert!(!is_current_chart(
        &versions,
        std::slice::from_ref(&source),
        ChartGeneration::Deluxe,
        Difficulty::Master,
    ));
    assert!(!is_current_chart(
        &["older".to_owned()],
        &[source],
        ChartGeneration::Standard,
        Difficulty::Master,
    ));
    Ok(())
}

#[test]
fn target_set_is_exactly_the_four_legacy_achievements() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        targets()?.map(AchievementRate::ten_thousandths),
        [990_000, 995_000, 1_000_000, 1_005_000]
    );
    Ok(())
}

#[test]
fn zero_score_filter_keeps_the_legacy_falsy_behavior() {
    assert!(passes_floor(260, 0, 252, 270, None));
    assert!(passes_floor(260, 0, 252, 270, Some(0)));
    assert!(!passes_floor(260, 0, 252, 270, Some(1)));
}

#[test]
fn legacy_random_selection_is_reproducible_with_an_injected_rng()
-> Result<(), Box<dyn std::error::Error>> {
    let candidates = vec![
        (0, render_candidate(10, ChartGeneration::Standard)?),
        (0, render_candidate(11, ChartGeneration::Deluxe)?),
        (0, render_candidate(12, ChartGeneration::Standard)?),
        (0, render_candidate(13, ChartGeneration::Deluxe)?),
        (0, render_candidate(14, ChartGeneration::Standard)?),
        (0, render_candidate(15, ChartGeneration::Deluxe)?),
    ];
    let mut first = FixedRandom::new([5, 0, 1, 0, 0]);
    let mut second = FixedRandom::new([5, 0, 1, 0, 0]);
    let first = select_legacy(candidates.clone(), &mut first);
    let second = select_legacy(candidates, &mut second);
    assert_eq!(first, second);
    assert_eq!(first.len(), 5);
    Ok(())
}

#[test]
fn display_order_uses_diving_fish_id_then_difficulty() -> Result<(), Box<dyn std::error::Error>> {
    let candidates = vec![
        (
            0,
            render_candidate_at(10, ChartGeneration::Deluxe, Difficulty::Expert)?,
        ),
        (
            0,
            render_candidate_at(9, ChartGeneration::Standard, Difficulty::Master)?,
        ),
        (
            0,
            render_candidate_at(10, ChartGeneration::Standard, Difficulty::Master)?,
        ),
    ];
    let mut rng = FixedRandom::new([]);
    let selected = select_legacy(candidates, &mut rng);
    assert_eq!(
        selected[0].display_id,
        maimai_core::SongIdValue::Numeric(10)
    );
    assert_eq!(selected[0].difficulty(), Difficulty::Master);
    assert_eq!(selected[1].difficulty(), Difficulty::Expert);
    assert_eq!(selected[2].display_id, maimai_core::SongIdValue::Numeric(9));
    Ok(())
}

#[test]
fn expected_weights_stay_bound_to_the_original_pool_rank() {
    let weights = (0..4).map(|rank| rank_weight(4, rank)).collect::<Vec<_>>();
    assert!(weights.windows(2).all(|pair| pair[0] > pair[1]));
    assert_eq!(weights[0], rank_weight(4, 0));
    assert_eq!(weights[3], rank_weight(4, 3));
}

#[test]
fn expected_sampling_is_reproducible_with_an_injected_rng() -> Result<(), Box<dyn std::error::Error>>
{
    let candidates = (20..28)
        .map(|id| {
            Ok(ExpectedPick {
                score: Decimal::from(id),
                gain: id,
                bucket: 1,
                item: render_candidate(id, ChartGeneration::Deluxe)?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let mut first = FixedRandom::new([1, 7, 3, 11, 5]);
    let mut second = FixedRandom::new([1, 7, 3, 11, 5]);
    assert_eq!(
        select_expected(candidates.clone(), &mut first),
        select_expected(candidates, &mut second)
    );
    Ok(())
}

fn records(
    count: usize,
    floor: u32,
    first_id: u32,
) -> Result<Vec<B50Chart>, Box<dyn std::error::Error>> {
    (0..count)
        .map(|index| {
            let id = first_id + u32::try_from(index)?;
            record(
                chart(
                    SourceSongId::numeric(SongIdNamespace::DivingFish, id),
                    ChartGeneration::Standard,
                    Difficulty::Master,
                )?,
                floor,
            )
        })
        .collect()
}

fn record(key: ChartKey, rating: u32) -> Result<B50Chart, Box<dyn std::error::Error>> {
    Ok(B50Chart {
        source_song_id: key.song().clone(),
        key,
        title: "song".to_owned(),
        level: "14".to_owned(),
        constant: Some(ChartConstant::from_decimal_str("14.0")?),
        achievements: Some(AchievementRate::from_decimal_str("100.0")?.into()),
        dx_score: None,
        rating: Some(rating),
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

fn chart(
    song: SourceSongId,
    generation: ChartGeneration,
    difficulty: Difficulty,
) -> Result<ChartKey, maimai_core::ValidationError> {
    ChartKey::new(song, generation, difficulty)
}

fn projection(
    version: &str,
    generation: ChartGeneration,
) -> Result<SourceSongProjection, Box<dyn std::error::Error>> {
    let id = SourceSongId::numeric(SongIdNamespace::DivingFish, 10);
    Ok(SourceSongProjection {
        source: SourceKind::DivingFish,
        id: id.clone(),
        title: "song".to_owned(),
        artist: String::new(),
        genre: String::new(),
        bpm: None,
        version: version.to_owned(),
        release_date: None,
        is_new: Some(true),
        is_locked: None,
        image_name: None,
        source_fields: SourceSongFields::default(),
        charts: vec![SourceChartProjection {
            source_song_id: id,
            generation,
            difficulty: Difficulty::Master,
            level: "14".to_owned(),
            constant: None,
            note_designer: String::new(),
            notes: None,
            note_total: None,
            version: version.to_owned(),
            regions: RegionAvailability {
                cn: true,
                ..RegionAvailability::default()
            },
            release_date: None,
            internal_id: None,
            fit_source_id: None,
            fit_stats: None,
            music_id: None,
            chart_id: None,
            is_buddy: None,
            kanji: None,
            description: None,
            raw_difficulty: None,
            is_special: false,
            region_overrides: BTreeMap::new(),
            multiver_constants: BTreeMap::new(),
        }],
    })
}

fn render_candidate(
    id: u32,
    generation: ChartGeneration,
) -> Result<maimai_render::RiseScoreCandidate, Box<dyn std::error::Error>> {
    render_candidate_at(id, generation, Difficulty::Master)
}

fn render_candidate_at(
    id: u32,
    generation: ChartGeneration,
    difficulty: Difficulty,
) -> Result<maimai_render::RiseScoreCandidate, Box<dyn std::error::Error>> {
    let key = chart(
        SourceSongId::numeric(SongIdNamespace::DivingFish, id),
        generation,
        difficulty,
    )?;
    Ok(maimai_render::RiseScoreCandidate {
        key,
        display_id: maimai_core::SongIdValue::Numeric(id),
        cover_id: maimai_core::SongIdValue::Numeric(id),
        image_name: None,
        title: format!("song {id}"),
        constant: ChartConstant::from_decimal_str("14.0")?,
        old_achievement: None,
        old_rating: 0,
        target_achievement: AchievementRate::from_decimal_str("99.0")?,
        target_rating: 280,
        gain: 280,
    })
}
