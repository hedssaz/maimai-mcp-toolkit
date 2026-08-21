use maimai_core::{
    ChartGeneration, ChartKey, Difficulty, Music, SongIdNamespace, SongIdValue, SourceSongId,
};

use super::{CatalogDocuments, CatalogError, CatalogSnapshot};
use crate::{CatalogLookupError, MatchKind, SourceKind};

impl CatalogSnapshot {
    pub(crate) fn from_test_songs(songs: Vec<Music>) -> Result<Self, CatalogError> {
        let normalizer = crate::TextNormalizer::default();
        let search_index = crate::search::SearchIndex::build(&songs, &normalizer, &[], &[], &[]);
        let mut metadata = crate::metadata::CatalogMetadata {
            songs: vec![Default::default(); songs.len()],
            charts: songs
                .iter()
                .map(|song| vec![Default::default(); song.charts.len()])
                .collect(),
            ..Default::default()
        };
        for (song, song_metadata) in songs.iter().zip(&mut metadata.songs) {
            for source_id in &song.source_ids {
                if let SongIdValue::Numeric(value) = source_id.value() {
                    song_metadata.canonical_numeric_ids.insert(*value);
                }
            }
        }
        Ok(Self {
            chart_index: crate::lookup::build_chart_index(&songs, &metadata)?,
            songs,
            normalizer,
            search_index,
            metadata,
            plates: crate::plate::PlateCatalog::default(),
        })
    }
}

const LXNS: &str = r#"
    {
      "songs": [{
        "id": 8,
        "title": "True Love Song",
        "artist": "Kai",
        "genre": "maimai",
        "bpm": 150,
        "version": 10000,
        "difficulties": {
          "standard": [{
            "difficulty": 3,
            "level": "12",
            "level_value": 12.4,
            "note_designer": "LXNS",
            "notes": {"tap": 263, "hold": 14, "slide": 19, "touch": 0, "break": 6}
          }],
          "utage": [{
            "difficulty": 0,
            "level": "宴",
            "level_value": 0,
            "note_designer": "宴",
            "notes": {"tap": 1}
          }]
        }
      }],
      "genres": [{"title": "舞萌", "genre": "maimai"}],
      "versions": [{"title": "maimai", "version": 10000}]
    }
    "#;

const DIVING_FISH: &str = r#"
    [
      {
        "id": "8",
        "title": "True Love Song",
        "type": "SD",
        "ds": [12.4],
        "level": ["12"],
        "charts": [{"notes": [263, 14, 19, 6], "charter": "DF-ST"}],
        "basic_info": {"artist": "Kai", "genre": "舞萌", "bpm": 150, "from": "maimai"}
      },
      {
        "id": "10008",
        "title": "True Love Song",
        "type": "DX",
        "ds": [13.7],
        "level": ["13+"],
        "charts": [{"notes": [300, 20, 40, 10, 5], "charter": "DF-DX"}],
        "basic_info": {"artist": "Kai", "genre": "舞萌", "bpm": 150, "from": "でらっくす"}
      },
      {
        "id": "100008",
        "title": "[宴]True Love Song",
        "type": "DX",
        "ds": [14.0],
        "level": ["14?"],
        "charts": [{"notes": [400, 30, 50, 20, 10], "charter": "宴"}],
        "basic_info": {"artist": "Kai", "genre": "宴会場", "bpm": 150, "from": "PRiSM"}
      }
    ]
    "#;

fn fixture() -> CatalogDocuments<'static> {
    CatalogDocuments {
        lxns_songs: LXNS,
        diving_fish_songs: DIVING_FISH,
        lxns_aliases: r#"{"aliases":[{"song_id":8,"aliases":["真爱"]}]}"#,
        yuzu_aliases: r#"{"content":[{"SongID":8,"Alias":["真爱歌"]}]}"#,
        custom_aliases: r#"{"8":["糖糖"]}"#,
        pinyin_aliases: r#"{"aliases":[{"song_id":"True Love Song","aliases":["zhenai","yulushuangxue","ylsx","xxch2"]}]}"#,
        simplified_to_traditional: r#"{"爱":"愛"}"#,
        dxdata: None,
        chart_stats: None,
        tags: None,
        official_music_data: None,
        dxrating_aliases: None,
        legacy_aliases_csv: None,
        artist_aliases: None,
        charter_aliases: None,
        traditional_to_simplified: None,
        maimaidxplate: None,
        custom_plates: None,
    }
}

#[test]
fn loads_all_alias_sources_and_searches_true_love() -> Result<(), CatalogError> {
    let snapshot = CatalogSnapshot::from_documents(&fixture())?;
    let hits = snapshot.search_text("真爱", 5);

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].music.title, "True Love Song");
    assert_eq!(hits[0].matched_by, MatchKind::ExactAlias);
    assert!(hits[0].music.aliases.iter().any(|alias| alias == "真爱歌"));
    assert!(hits[0].music.aliases.iter().any(|alias| alias == "糖糖"));
    assert!(!hits[0].music.aliases.iter().any(|alias| alias == "zhenai"));
    let pinyin_hits = snapshot.search_text("zhenai", 5);
    assert_eq!(pinyin_hits.len(), 1);
    assert_eq!(pinyin_hits[0].matched_by, MatchKind::PinyinExact);
    let homophone_hits = snapshot.search_text("语录爽学", 5);
    assert_eq!(homophone_hits[0].matched_by, MatchKind::PinyinExact);
    assert_eq!(
        homophone_hits[0].matched_value.as_deref(),
        Some("yulushuangxue")
    );
    assert_eq!(
        snapshot.search_text("ylsx", 5)[0].matched_by,
        MatchKind::PinyinExact
    );
    assert_eq!(
        snapshot.search_text("xxch2", 5)[0].matched_by,
        MatchKind::PinyinExact
    );
    Ok(())
}

#[test]
fn merges_standard_and_normal_dx_with_source_ids() -> Result<(), CatalogError> {
    let snapshot = CatalogSnapshot::from_documents(&fixture())?;
    let music = snapshot
        .songs()
        .iter()
        .find(|music| music.title == "True Love Song")
        .ok_or(CatalogError::EmptyDivingFishSongId)?;

    assert_eq!(snapshot.songs().len(), 2);
    assert!(
        music
            .charts
            .iter()
            .any(|chart| chart.key.generation() == ChartGeneration::Standard)
    );
    assert!(
        music
            .charts
            .iter()
            .any(|chart| chart.key.generation() == ChartGeneration::Deluxe)
    );
    assert!(music.source_ids.iter().any(|source_id| {
        source_id.namespace() == SongIdNamespace::DivingFish
            && source_id.value() == &SongIdValue::Numeric(10_008)
    }));
    Ok(())
}

#[test]
fn lxns_utage_bucket_uses_utage_difficulty_not_basic() -> Result<(), CatalogError> {
    let snapshot = CatalogSnapshot::from_documents(&fixture())?;
    let music = &snapshot.songs()[0];
    assert!(music.charts.iter().any(|chart| {
        chart.key.generation() == ChartGeneration::UtageOnePlayer
            && chart.key.difficulty() == maimai_core::Difficulty::Utage
    }));
    assert!(!music.charts.iter().any(|chart| {
        chart.key.generation() == ChartGeneration::UtageOnePlayer
            && chart.key.difficulty() == maimai_core::Difficulty::Basic
    }));
    Ok(())
}

#[test]
fn keeps_utage_id_outside_normal_dx_offset_space() -> Result<(), CatalogError> {
    let snapshot = CatalogSnapshot::from_documents(&fixture())?;
    let utage = snapshot
        .songs()
        .iter()
        .find(|music| music.title == "[宴]True Love Song")
        .ok_or(CatalogError::EmptyDivingFishSongId)?;

    assert_eq!(utage.primary_id.value(), &SongIdValue::Numeric(100_008));
    assert!(utage.charts.iter().all(|chart| {
        chart.key.generation() == ChartGeneration::UtageOnePlayer
            && chart.key.song().value() == &SongIdValue::Numeric(100_008)
            && chart.constant.is_none()
    }));
    Ok(())
}

#[test]
fn source_chart_uses_canonical_key_and_keeps_dx_offset_in_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let snapshot = CatalogSnapshot::from_documents(&fixture())?;
    let key = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::Lxns, 8),
        ChartGeneration::Deluxe,
        Difficulty::Basic,
    )?;
    let source = snapshot
        .source_chart(&key, SourceKind::DivingFish)?
        .ok_or("missing Diving-Fish DX projection")?;

    assert_eq!(source.song.id.value(), &SongIdValue::Numeric(10_008));
    assert_eq!(
        source.chart.source_song_id.value(),
        &SongIdValue::Numeric(10_008)
    );
    assert!(snapshot.source_chart(&key, SourceKind::Japan)?.is_none());
    Ok(())
}

#[test]
fn source_chart_rejects_noncanonical_namespace() -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = CatalogSnapshot::from_documents(&fixture())?;
    let key = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::OfficialCn, 8),
        ChartGeneration::Standard,
        Difficulty::Basic,
    )?;
    assert_eq!(
        snapshot.source_chart(&key, SourceKind::Official),
        Err(CatalogLookupError::UnsupportedChartNamespace {
            namespace: SongIdNamespace::OfficialCn,
        })
    );
    Ok(())
}

#[test]
fn source_chart_resolves_real_cross_namespace_ids_without_global_offset_guessing()
-> Result<(), Box<dyn std::error::Error>> {
    let data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let snapshot = super::CatalogFiles::main_from_data_dir(data).load()?;
    let link = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::Lxns, 383),
        ChartGeneration::Standard,
        Difficulty::Master,
    )?;
    let link_df = snapshot
        .source_chart(&link, SourceKind::DivingFish)?
        .ok_or("Link Diving-Fish projection missing")?;
    assert_eq!(link_df.song.id.value(), &SongIdValue::Numeric(383));

    let nonexistent_link_offset = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::DivingFish, 10_383),
        ChartGeneration::Deluxe,
        Difficulty::Master,
    )?;
    assert!(
        snapshot
            .source_chart(&nonexistent_link_offset, SourceKind::China)?
            .is_none()
    );

    let destined_df = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::DivingFish, 11_855),
        ChartGeneration::Deluxe,
        Difficulty::Master,
    )?;
    let destined_cn = snapshot
        .source_chart(&destined_df, SourceKind::China)?
        .ok_or("Destined Marionette LXNS projection missing")?;
    assert_eq!(destined_cn.song.id.value(), &SongIdValue::Numeric(1_855));
    Ok(())
}
