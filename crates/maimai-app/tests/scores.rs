use std::{error::Error, fs};

use maimai_catalog::{CatalogFiles, CatalogSnapshot};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, PlayerSelector, QqId,
    RatingBreakdown, ScoreSource, SongIdNamespace, SourceSongId, single_song_rating,
};
use maimai_providers::{
    DivingFishB50, DivingFishChartGeneration, DivingFishPlayer, DivingFishPlayerRecords,
    DivingFishScore, DivingFishScoreCounts, LxnsPlayerBests, LxnsPlayerScores, LxnsScore,
};
use maimai_storage::PlayerRecord;
use serde_json::json;
use tempfile::TempDir;

use maimai_app::scores::{
    B50Chart, FitIndexLabel, Lookup, RatingMode, ScoreErrorCode, SelectionReason, SongFilter,
    SongGenerationFilter, compute_b50_from_records, compute_fit_index, filter_single_song,
    from_diving_fish_b50, from_diving_fish_records, from_local_records, from_lxns_bests,
    from_lxns_scores, select_source,
};

#[test]
fn source_selection_has_no_implicit_fallback() -> Result<(), Box<dyn Error>> {
    let qq = Lookup::Qq(QqId::new("10001")?);
    let explicit = select_source(&qq, Some(ScoreSource::Lxns), Some(ScoreSource::Local))?;
    assert_eq!(explicit.source, ScoreSource::Lxns);
    assert_eq!(explicit.preferred_source, ScoreSource::Local);
    assert_eq!(explicit.reason, SelectionReason::ExplicitOverride);
    let preferred = select_source(&qq, None, Some(ScoreSource::Local))?;
    assert_eq!(preferred.source, ScoreSource::Local);
    assert_eq!(preferred.preferred_source, ScoreSource::Local);
    assert_eq!(preferred.reason, SelectionReason::QqPreference);
    let username = Lookup::Username(maimai_core::PlayerUsername::new("tester")?);
    let fixed = select_source(&username, Some(ScoreSource::Local), None)?;
    assert_eq!(fixed.source, ScoreSource::DivingFish);
    assert_eq!(fixed.reason, SelectionReason::UsernameFixed);
    let error = select_source(&qq, Some(ScoreSource::OfficialCn), None)
        .err()
        .ok_or("expected unsupported source")?;
    assert_eq!(error.code(), ScoreErrorCode::UnsupportedSource);
    Ok(())
}

#[test]
fn fit_index_keeps_fifty_charts_sixteen_decimal_places_and_negative_delta_exact()
-> Result<(), Box<dyn Error>> {
    let charts = (1..=50).map(fit_chart).collect::<Result<Vec<_>, _>>()?;
    let index = compute_fit_index(&charts[..35], &charts[35..]);
    assert_eq!(index.b50.counted, 50);
    assert_eq!(index.b50.missing, 0);
    assert_eq!(index.b50.virtual_rating, Some(-250));
    let ratio = index.b50.virtual_ratio_percent.ok_or("missing ratio")?;
    assert_eq!((ratio.numerator(), ratio.denominator()), (-500, 303));
    let delta = index
        .b50
        .weighted_average_delta
        .ok_or("missing weighted delta")?;
    assert_eq!((delta.numerator(), delta.denominator()), (-1, 5));
    assert_eq!(index.label, Some(FitIndexLabel::ClearlyDeflated));
    Ok(())
}

#[test]
fn utage_any_generation_includes_one_and_two_player_charts() {
    let filter = SongGenerationFilter::UtageAny;
    assert!(filter.matches(ChartGeneration::UtageOnePlayer));
    assert!(filter.matches(ChartGeneration::UtageTwoPlayer));
    assert!(!filter.matches(ChartGeneration::Deluxe));
}

#[test]
fn three_sources_normalize_to_one_stable_chart_with_exact_decimals() -> Result<(), Box<dyn Error>> {
    let fixture = catalog()?;
    let lookup = qq_lookup()?;
    let mut df_record = df_score(
        10_383,
        "Link(CoF)",
        DivingFishChartGeneration::Deluxe,
        "13.8",
        "100.1234",
        1,
    )?;
    df_record.version = Some("provider-version".to_owned());
    let df = from_diving_fish_records(
        DivingFishPlayerRecords {
            lookup: PlayerSelector::Qq(QqId::new("10001")?),
            player: df_player(),
            records: vec![df_record],
        },
        &fixture,
    )?;
    let lxns_score = lxns_score(383, "dx", 3, "13.8", "100.1234")?;
    let lxns = from_lxns_scores(
        lookup.clone(),
        LxnsPlayerScores {
            player: None,
            scores: vec![lxns_score],
        },
        &fixture,
    )?;
    let mut local_record = local_record(
        ChartGeneration::Deluxe,
        Difficulty::Master,
        "13.8",
        "100.1234",
        1,
        true,
    )?;
    local_record.version = Some("local-record-version".to_owned());
    let local = from_local_records(lookup, &[local_record], None, &fixture)?;

    let expected_rating = single_song_rating(ds("13.8")?, ach("100.1234")?)?;
    for record in [&df.records[0], &lxns.records[0], &local.records[0]] {
        assert_eq!(record.key.song(), &lxns_id(383));
        assert_eq!(record.key.generation(), ChartGeneration::Deluxe);
        assert_eq!(record.title, "Link");
        assert_eq!(record.rating, Some(expected_rating));
        assert_eq!(
            record
                .achievements
                .and_then(|value| value.ranked())
                .map(AchievementRate::ten_thousandths),
            Some(1_001_234)
        );
    }
    assert_eq!(df.records[0].source_song_id, df_id(10_383));
    assert_eq!(lxns.records[0].source_song_id, lxns_id(383));
    assert_eq!(df.records[0].version, "provider-version");
    assert_eq!(local.records[0].version, "local-record-version");
    assert_eq!(lxns.records[0].version, "PRiSM");
    assert!(!local.records[0].is_current);
    Ok(())
}

#[test]
fn diving_fish_and_lxns_bests_share_partial_b50_shape() -> Result<(), Box<dyn Error>> {
    let fixture = catalog()?;
    let old = df_score(
        10_383,
        "Link(CoF)",
        DivingFishChartGeneration::Deluxe,
        "13.8",
        "100.0000",
        1,
    )?;
    let new = df_score(
        10_384,
        "New Song",
        DivingFishChartGeneration::Deluxe,
        "14.2",
        "100.5000",
        1,
    )?;
    let df = from_diving_fish_b50(
        DivingFishB50 {
            lookup: PlayerSelector::Qq(QqId::new("10001")?),
            player: df_player(),
            counts: DivingFishScoreCounts {
                sd: 1,
                dx: 1,
                total: 2,
            },
            rating_breakdown: RatingBreakdown {
                b35: 777,
                b15: 333,
                total: 1_110,
            },
            sd: vec![old],
            dx: vec![new],
        },
        &fixture,
    )?;
    let lxns = from_lxns_bests(
        qq_lookup()?,
        LxnsPlayerBests {
            player: None,
            standard: vec![lxns_score(383, "dx", 3, "13.8", "100.0000")?],
            deluxe: vec![lxns_score(384, "dx", 3, "14.2", "100.5000")?],
            standard_total: Some(777),
            deluxe_total: Some(333),
        },
        &fixture,
    )?;
    assert_eq!(df.total_count(), 2);
    assert_eq!(lxns.total_count(), 2);
    assert_eq!(df.b35[0].key, lxns.b35[0].key);
    assert_eq!(df.b15[0].key, lxns.b15[0].key);
    assert_eq!(df.rating_breakdown, lxns.rating_breakdown);
    assert_eq!(df.rating_breakdown.total, 1_110);
    assert_ne!(
        df.rating_breakdown.b35,
        df.b35[0].rating.unwrap_or_default()
    );
    Ok(())
}

#[test]
fn catalog_current_version_and_fit_mode_drive_b35_b15_and_stable_sort() -> Result<(), Box<dyn Error>>
{
    let fixture = catalog()?;
    assert_eq!(
        fixture.current_diving_fish_versions(),
        &["DF PRiSM PLUS".to_owned()]
    );
    assert_eq!(fixture.latest_cn_versions(), &[27_000]);
    let input = from_lxns_scores(
        qq_lookup()?,
        LxnsPlayerScores {
            player: Some(serde_json::from_value(json!({
                "name": "Tester",
                "rating": 15_000
            }))?),
            scores: vec![
                lxns_score(383, "standard", 3, "13.0", "100.0000")?,
                lxns_score(383, "dx", 3, "13.8", "100.1000")?,
                lxns_score(383, "dx", 3, "13.8", "99.0000")?,
                lxns_score(384, "standard", 3, "14.0", "100.0000")?,
                lxns_score(383, "utage", 0, "0", "100.0000")?,
            ],
        },
        &fixture,
    )?;
    let actual = compute_b50_from_records(&input, RatingMode::Actual)?;
    assert_eq!(actual.b35.len(), 2);
    assert_eq!(actual.b15.len(), 1);
    assert_eq!(actual.b15[0].title, "New Song");
    assert_eq!(actual.player.actual_rating, Some(15_000));
    assert_eq!(actual.player.rating, Some(actual.rating_breakdown.total));
    let actual_stats = actual.computation.ok_or("missing computation")?;
    assert_eq!(actual_stats.duplicate_lower_rating, 1);
    assert_eq!(actual_stats.skipped_utage, 1);

    let fit = compute_b50_from_records(&input, RatingMode::Fit)?;
    assert_eq!(fit.b35[0].fit_constant, Some(ds("13.5")?));
    assert_eq!(fit.b35[0].original_rating, input.records[1].rating);
    assert_eq!(
        fit.b35[0].rating,
        Some(single_song_rating(ds("13.5")?, ach("100.1000")?)?)
    );
    assert_eq!(fit.b15[0].fit_constant, Some(ds("14.1")?));
    assert_eq!(fit.total_count(), 3);
    Ok(())
}

#[test]
fn single_song_filter_uses_id_and_keeps_both_generations() -> Result<(), Box<dyn Error>> {
    let fixture = catalog()?;
    let input = from_diving_fish_records(
        DivingFishPlayerRecords {
            lookup: PlayerSelector::Qq(QqId::new("10001")?),
            player: df_player(),
            records: vec![
                df_score(
                    383,
                    "Link old title",
                    DivingFishChartGeneration::Standard,
                    "13.0",
                    "100.0000",
                    1,
                )?,
                df_score(
                    10_383,
                    "Link(CoF)",
                    DivingFishChartGeneration::Deluxe,
                    "13.8",
                    "100.0000",
                    1,
                )?,
                df_score(
                    384,
                    "New Song",
                    DivingFishChartGeneration::Standard,
                    "14.0",
                    "100.0000",
                    1,
                )?,
            ],
        },
        &fixture,
    )?;
    let records = filter_single_song(&input, &SongFilter::new(lxns_id(383)), &fixture)?;
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.title == "Link"));
    assert_eq!(records[0].key.generation(), ChartGeneration::Standard);
    assert_eq!(records[1].key.generation(), ChartGeneration::Deluxe);
    Ok(())
}

fn catalog() -> Result<CatalogSnapshot, Box<dyn Error>> {
    let directory = TempDir::new()?;
    write(
        &directory,
        "lxns_song_list.json",
        &json!({
            "songs": [
                song(383, "Link", 25000, true),
                song(384, "New Song", 26000, false),
                song(385, "LXNS Future Song", 27000, false)
            ],
            "genres": [{"title":"舞萌","genre":"maimai"}],
            "versions": [
                {"title":"PRiSM","version":25000},
                {"title":"PRiSM PLUS","version":26000},
                {"title":"Future Numeric Version","version":27000}
            ]
        })
        .to_string(),
    )?;
    write(
        &directory,
        "divingfish_song_list.json",
        &json!([
            df_song(383, "Link(CoF)", "SD", "13.0", "PRiSM"),
            df_song(10383, "Link(CoF)", "DX", "13.8", "PRiSM"),
            df_song(384, "New Song", "SD", "14.0", "PRiSM PLUS"),
            df_song(10384, "New Song", "DX", "14.2", "PRiSM PLUS")
        ])
        .to_string(),
    )?;
    write(
        &directory,
        "divingfish_chart_stats.json",
        &json!({"charts": {
            "383": [{},{},{},{"fit_diff":12.9,"diff":"13"}],
            "10383": [{},{},{},{"fit_diff":13.5,"diff":"13+"}],
            "384": [{},{},{},{"fit_diff":14.1,"diff":"14"}],
            "10384": [{},{},{},{"fit_diff":14.0,"diff":"14+"}]
        }})
        .to_string(),
    )?;
    for (name, contents) in [
        ("lxns_alias_list.json", r#"{"aliases":[]}"#),
        ("music_alias.json", r#"{"content":[]}"#),
        ("custom_aliases.json", "{}"),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#),
        ("zh_s2t.json", "{}"),
    ] {
        write(&directory, name, contents)?;
    }
    let result = CatalogFiles::from_data_dir(directory.path()).load()?;
    Ok(result)
}

fn song(id: u32, title: &str, version: u32, include_utage: bool) -> serde_json::Value {
    let mut difficulties = json!({
        "standard": [{"difficulty":3,"level":if id == 383 {"13"} else {"14"},
            "level_value":if id == 383 {13.0} else {14.0},"notes":{}}],
        "dx": [{"difficulty":3,"level":if id == 383 {"13+"} else {"14+"},
            "level_value":if id == 383 {13.8} else {14.2},"notes":{}}]
    });
    if include_utage {
        difficulties["utage"] = json!([{
            "difficulty":0,"level":"宴","level_value":0,"notes":{}
        }]);
    }
    json!({
        "id":id,"title":title,"artist":"Artist","genre":"maimai","bpm":150,
        "version":version,"difficulties":difficulties
    })
}

fn df_song(id: u32, title: &str, kind: &str, master_ds: &str, version: &str) -> serde_json::Value {
    let numeric_ds = match master_ds {
        "13.0" => json!(13.0),
        "13.8" => json!(13.8),
        "14.0" => json!(14.0),
        "14.2" => json!(14.2),
        _ => json!(0),
    };
    json!({
        "id":id.to_string(),"title":title,"type":kind,
        "ds":[1.0,2.0,3.0,numeric_ds],"level":["1","2","3", if master_ds == "13.8" {"13+"} else if master_ds == "14.2" {"14+"} else {master_ds}],
        "charts":[{"notes":[]},{"notes":[]},{"notes":[]},{"notes":[]}],
        "basic_info":{"artist":"Artist","genre":"舞萌","bpm":150,"from":format!("DF {version}"),"is_new":version == "PRiSM PLUS"}
    })
}

fn write(directory: &TempDir, name: &str, contents: &str) -> Result<(), std::io::Error> {
    fs::write(directory.path().join(name), contents)
}

fn df_score(
    id: u32,
    title: &str,
    generation: DivingFishChartGeneration,
    constant: &str,
    achievements: &str,
    rating: u32,
) -> Result<DivingFishScore, Box<dyn Error>> {
    Ok(DivingFishScore {
        song_id: df_id(id),
        title: title.to_owned(),
        generation,
        difficulty: Difficulty::Master,
        level: constant.to_owned(),
        constant: Some(ds(constant)?),
        achievements: Some(ach(achievements)?.into()),
        dx_score: Some(1_000),
        rating: Some(rating),
        grade: None,
        full_combo: None,
        full_sync: None,
        version: None,
    })
}

fn lxns_score(
    id: u32,
    kind: &str,
    difficulty: u8,
    constant: &str,
    achievements: &str,
) -> Result<LxnsScore, serde_json::Error> {
    serde_json::from_value(json!({
        "id":id,"type":kind,"level_index":difficulty,
        "achievements":achievements,"dx_score":1000,"ds":constant
    }))
}

fn local_record(
    generation: ChartGeneration,
    difficulty: Difficulty,
    constant: &str,
    achievements: &str,
    rating: i64,
    is_new: bool,
) -> Result<PlayerRecord, Box<dyn Error>> {
    Ok(PlayerRecord {
        qq: QqId::new("10001")?,
        chart: ChartKey::new(lxns_id(383), generation, difficulty)?,
        title: "stale local title".to_owned(),
        level: Some(constant.to_owned()),
        level_label: Some("Master".to_owned()),
        ds: Some(ds(constant)?),
        achievements: Some(ach(achievements)?.into()),
        dx_score: Some(1000),
        fc: None,
        fs: None,
        rate: None,
        ra: Some(rating),
        version: None,
        is_new,
        score_source: ScoreSource::DivingFish,
        source_detail: None,
        raw: None,
        payload: json!({}),
        updated_at: "2026-08-18T00:00:00Z".to_owned(),
    })
}

fn df_player() -> DivingFishPlayer {
    DivingFishPlayer {
        nickname: Some("Tester".to_owned()),
        username: None,
        rating: Some(15_000),
        additional_rating: Some(10),
        plate: Some("舞舞".to_owned()),
    }
}

fn qq_lookup() -> Result<Lookup, maimai_core::ValidationError> {
    Ok(Lookup::Qq(QqId::new("10001")?))
}

fn ds(value: &str) -> Result<ChartConstant, maimai_core::RatingError> {
    ChartConstant::from_decimal_str(value)
}

fn ach(value: &str) -> Result<AchievementRate, maimai_core::RatingError> {
    AchievementRate::from_decimal_str(value)
}

fn lxns_id(value: u32) -> SourceSongId {
    SourceSongId::numeric(SongIdNamespace::Lxns, value)
}

fn df_id(value: u32) -> SourceSongId {
    SourceSongId::numeric(SongIdNamespace::DivingFish, value)
}

fn fit_chart(id: u32) -> Result<B50Chart, Box<dyn Error>> {
    let song = df_id(id);
    Ok(B50Chart {
        key: ChartKey::new(
            song.clone(),
            if id <= 35 {
                ChartGeneration::Standard
            } else {
                ChartGeneration::Deluxe
            },
            Difficulty::Master,
        )?,
        source_song_id: song,
        title: format!("Exact {id}"),
        level: "13+".to_owned(),
        constant: Some(ds("13.5000000000000000")?),
        achievements: Some(ach("100.5000")?.into()),
        dx_score: None,
        rating: Some(303),
        original_rating: None,
        grade: None,
        full_combo: None,
        full_sync: None,
        version: "Old".to_owned(),
        is_current: id > 35,
        fit_constant: Some(ds("13.7000000000000000")?),
        fit_label: None,
    })
}
