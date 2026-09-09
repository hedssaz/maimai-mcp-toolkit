use std::error::Error;

use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, PlayAchievement,
    SongIdNamespace, SourceSongId, UtageScore,
};
use maimai_render::{MusicInfoChart, MusicInfoView};

use super::prepare::view;
use crate::{
    music_info::resolve::PreparedMusicInfo,
    scores::{B50Chart, best_by_chart},
};

#[test]
fn five_rows_use_stable_best_record_and_zero_notes_stay_safe() -> Result<(), Box<dyn Error>> {
    let song = SourceSongId::numeric(SongIdNamespace::Lxns, 383);
    let difficulties = [
        Difficulty::Basic,
        Difficulty::Advanced,
        Difficulty::Expert,
        Difficulty::Master,
        Difficulty::ReMaster,
    ];
    let mut charts = Vec::new();
    let mut keys = Vec::new();
    for difficulty in difficulties {
        charts.push(MusicInfoChart::new(
            difficulty,
            "13+",
            Some(ChartConstant::from_decimal_str("13.7")?),
            None,
            None,
            Some(0),
            "Charter",
        )?);
        keys.push(ChartKey::new(
            song.clone(),
            ChartGeneration::Deluxe,
            difficulty,
        )?);
    }
    let prepared = PreparedMusicInfo {
        view: music_view(song.clone(), charts)?,
        chart_keys: keys.clone(),
        generation: ChartGeneration::Deluxe,
        current: true,
        chart_type: Some(crate::music_info::MusicInfoChartType::Deluxe),
        query: "Link".to_owned(),
        music_id: "10383".to_owned(),
        title: "Link".to_owned(),
    };
    let lower = record(keys[3].clone(), "99.9999", 900, 280)?;
    let best = record(keys[3].clone(), "100.0000", 800, 281)?;
    let mut records = vec![lower, best];
    let selected = best_by_chart(&records);
    let rendered = view(&prepared, &selected)?;
    assert_eq!(rendered.rows.len(), 5);
    assert!(!rendered.rows[0].played);
    assert!(rendered.rows[3].played);
    assert_eq!(rendered.rows[3].rating, Some(281));
    assert_eq!(rendered.rows[3].theoretical_dx_score(), None);
    assert_eq!(rendered.rows[3].stars(), None);
    for (grade, expected) in [
        ("", "SSS"),
        ("　", "SSS"),
        ("\t", "SSS"),
        (" SSSP ", "SSS+"),
        ("future\ngrade", "future\ngrade"),
    ] {
        records[1].grade = Some(grade.to_owned());
        let rendered = view(&prepared, &best_by_chart(&records))?;
        assert_eq!(rendered.rows[3].grade.as_deref(), Some(expected));
        assert_eq!(records[1].grade.as_deref(), Some(grade));
    }
    Ok(())
}

#[test]
fn utage_row_keeps_exact_play_achievement_without_rank_grade() -> Result<(), Box<dyn Error>> {
    let song = SourceSongId::numeric(SongIdNamespace::Lxns, 100_001);
    let chart = MusicInfoChart::new(Difficulty::Utage, "宴", None, None, None, None, "Charter")?;
    let key = ChartKey::new(
        song.clone(),
        ChartGeneration::UtageOnePlayer,
        Difficulty::Utage,
    )?;
    let prepared = PreparedMusicInfo {
        view: music_view(song, vec![chart])?,
        chart_keys: vec![key.clone()],
        generation: ChartGeneration::UtageOnePlayer,
        current: false,
        chart_type: None,
        query: "宴会场".to_owned(),
        music_id: "100001".to_owned(),
        title: "宴会场".to_owned(),
    };
    let achievement = PlayAchievement::from(UtageScore::from_ten_thousandths(1_535_756));
    let records = vec![B50Chart {
        source_song_id: key.song().clone(),
        key,
        title: "宴会场".to_owned(),
        level: "宴".to_owned(),
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
    }];
    let rendered = view(&prepared, &best_by_chart(&records))?;
    assert!(rendered.is_utage());
    assert_eq!(rendered.rows.len(), 1);
    assert!(rendered.rows[0].played);
    assert_eq!(rendered.rows[0].achievement, Some(achievement));
    assert_eq!(rendered.rows[0].grade, None);
    assert_eq!(rendered.rows[0].rating, None);
    Ok(())
}

fn music_view(
    song: SourceSongId,
    charts: Vec<MusicInfoChart>,
) -> Result<MusicInfoView, maimai_render::RenderError> {
    MusicInfoView::new(
        Some(song),
        Some(383),
        "Link",
        "Artist",
        "maimai",
        "PRiSM",
        Some(150),
        Some(ChartGeneration::Deluxe),
        false,
        None,
        charts,
    )
}

fn record(
    key: ChartKey,
    achievement: &str,
    dx_score: u32,
    rating: u32,
) -> Result<B50Chart, Box<dyn Error>> {
    Ok(B50Chart {
        source_song_id: key.song().clone(),
        key,
        title: "Link".to_owned(),
        level: "13+".to_owned(),
        constant: Some(ChartConstant::from_decimal_str("13.7")?),
        achievements: Some(AchievementRate::from_decimal_str(achievement)?.into()),
        dx_score: Some(dx_score),
        rating: Some(rating),
        original_rating: None,
        grade: Some("sss".to_owned()),
        full_combo: None,
        full_sync: None,
        version: "PRiSM".to_owned(),
        is_current: true,
        fit_constant: None,
        fit_label: None,
    })
}
