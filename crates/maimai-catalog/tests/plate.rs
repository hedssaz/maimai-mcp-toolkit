use std::{error::Error, path::PathBuf};

use maimai_catalog::{CatalogFiles, PlateName, PlateQuery, PlateServer};
use maimai_core::{ChartGeneration, Difficulty, SongIdValue};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

#[test]
fn real_cn_plate_data_keeps_aliases_grouping_and_unknown_semantics() -> TestResult {
    let snapshot = CatalogFiles::from_data_dir(data_dir()).load()?;
    let bear = snapshot.plate_membership(&PlateQuery::new(PlateName::new("熊")?, PlateServer::Cn));
    let traditional =
        snapshot.plate_membership(&PlateQuery::new(PlateName::new("華")?, PlateServer::Cn));
    let simplified =
        snapshot.plate_membership(&PlateQuery::new(PlateName::new("华")?, PlateServer::Cn));
    assert_eq!(bear.song_count(), 87);
    assert_eq!(traditional.song_count(), 87);
    assert_eq!(simplified.song_count(), 87);
    assert!(bear.matches(Some(10_191), "irrelevant", ChartGeneration::Standard));
    assert!(!bear.matches(Some(8), "True Love Song", ChartGeneration::Standard));

    let initial =
        snapshot.plate_membership(&PlateQuery::new(PlateName::new("初")?, PlateServer::Cn));
    let unknown =
        snapshot.plate_membership(&PlateQuery::new(PlateName::new("不存在")?, PlateServer::Cn));
    assert_eq!(initial.song_count(), 0);
    assert!(unknown.is_empty());
    Ok(())
}

#[test]
fn real_jp_membership_uses_old_cn_ids_and_exact_dxdata_versions() -> TestResult {
    let snapshot = CatalogFiles::from_data_dir(data_dir()).load()?;
    let initial =
        snapshot.plate_membership(&PlateQuery::new(PlateName::new("初")?, PlateServer::Jp));
    assert_eq!(initial.song_count(), 85);
    assert!(initial.matches(Some(8), "irrelevant", ChartGeneration::Deluxe));

    let bear = snapshot.plate_membership(&PlateQuery::new(PlateName::new("熊")?, PlateServer::Jp));
    let flower =
        snapshot.plate_membership(&PlateQuery::new(PlateName::new("華")?, PlateServer::Jp));
    let circle =
        snapshot.plate_membership(&PlateQuery::new(PlateName::new("circle")?, PlateServer::Jp));
    assert_eq!(bear.song_count(), 94);
    assert_eq!(flower.song_count(), 66);
    assert_eq!(circle.song_count(), 89);
    assert!(bear.matches(Some(10_146), "wrong", ChartGeneration::Standard));
    assert!(bear.matches(None, "39", ChartGeneration::Deluxe));
    assert!(!bear.matches(None, "39", ChartGeneration::Standard));
    assert!(!flower.matches(None, "39", ChartGeneration::Deluxe));
    Ok(())
}

#[test]
fn plate_name_rejects_empty_and_control_characters() {
    assert!(PlateName::new("").is_err());
    assert!(PlateName::new("  ").is_err());
    assert!(PlateName::new("熊\n").is_err());
}

#[test]
fn ordered_members_preserve_cn_jp_and_custom_source_identity() -> TestResult {
    let snapshot = CatalogFiles::from_data_dir(data_dir()).load()?;
    let cn = snapshot.plate_members(&PlateQuery::new(PlateName::new("熊")?, PlateServer::Cn));
    assert_eq!(cn.declared_song_count(), 87);
    assert!(!cn.members().is_empty());

    let jp = snapshot.plate_members(&PlateQuery::new(PlateName::new("熊")?, PlateServer::Jp));
    let thirty_nine = jp
        .members()
        .iter()
        .find(|member| member.title() == "39")
        .ok_or("JP 熊 should contain 39")?;
    assert_eq!(
        thirty_nine.identity().display_id(),
        &SongIdValue::Numeric(10_146)
    );
    assert_eq!(thirty_nine.generation(), ChartGeneration::Deluxe);

    let custom = snapshot.plate_members(&PlateQuery::new(
        PlateName::new("雪峰")?,
        PlateServer::Custom,
    ));
    assert_eq!(
        custom
            .members()
            .iter()
            .map(|member| member.identity().display_id())
            .collect::<Vec<_>>(),
        [
            &SongIdValue::Numeric(11_231),
            &SongIdValue::Numeric(11_752),
            &SongIdValue::Numeric(11_512),
            &SongIdValue::Numeric(11_630),
        ]
    );
    assert!(custom.members().iter().all(|member| {
        member
            .charts()
            .iter()
            .all(|chart| chart.difficulty() != Difficulty::ReMaster)
    }));
    Ok(())
}

#[test]
fn only_dance_plate_members_opt_into_whitelisted_remaster_charts() -> TestResult {
    let snapshot = CatalogFiles::from_data_dir(data_dir()).load()?;
    let dance = snapshot.plate_members(&PlateQuery::new(PlateName::new("舞")?, PlateServer::Cn));
    assert!(dance.members().iter().any(|member| {
        member
            .charts()
            .iter()
            .any(|chart| chart.difficulty() == Difficulty::ReMaster)
    }));
    let normal = snapshot.plate_members(&PlateQuery::new(PlateName::new("真")?, PlateServer::Cn));
    assert!(normal.members().iter().all(|member| {
        member
            .charts()
            .iter()
            .all(|chart| chart.difficulty() != Difficulty::ReMaster)
    }));
    Ok(())
}
