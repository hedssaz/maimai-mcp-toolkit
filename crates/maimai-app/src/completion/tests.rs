use std::path::PathBuf;

use maimai_catalog::{CatalogFiles, PlateName, PlateQuery, PlateServer};
use maimai_core::{AchievementRate, ScoreSource};

use crate::scores::{B50Chart, Lookup, PlayerScoreProfile, PlayerScores};

use super::{CompletionTarget, PlateSpec, plate, plate_progress, progress::retain_page};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn snapshot() -> Result<maimai_catalog::CatalogSnapshot, maimai_catalog::CatalogError> {
    CatalogFiles::from_data_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")).load()
}

fn empty_scores(lookup: Lookup) -> PlayerScores {
    PlayerScores {
        lookup,
        source: ScoreSource::DivingFish,
        player: PlayerScoreProfile::default(),
        records: Vec::new(),
    }
}

#[test]
fn prepares_real_cn_jp_and_custom_plate_views() -> TestResult {
    let snapshot = snapshot()?;
    let scores = empty_scores(Lookup::Qq(maimai_core::QqId::new("123456")?));
    for (version, server) in [
        ("真", PlateServer::Cn),
        ("丸", PlateServer::Jp),
        ("雪峰", PlateServer::Custom),
    ] {
        let prepared = plate::prepare(
            &snapshot,
            &scores,
            &PlateSpec {
                version: PlateName::new(version)?,
                target: CompletionTarget::PlateGeneral,
                server: Some(server),
            },
            true,
        )?;
        assert_eq!(prepared.server, server);
        assert!(!prepared.view.members.is_empty());
        assert_eq!(prepared.view.target, "将");
    }
    Ok(())
}

#[test]
fn completed_custom_plate_keeps_exact_legacy_text_golden() -> TestResult {
    let snapshot = snapshot()?;
    let spec = PlateSpec {
        version: PlateName::new("雪峰")?,
        target: CompletionTarget::PlateGeneral,
        server: Some(PlateServer::Custom),
    };
    let members =
        snapshot.plate_members(&PlateQuery::new(spec.version.clone(), PlateServer::Custom));
    let mut scores = empty_scores(Lookup::Username(maimai_core::PlayerUsername::new("alice")?));
    for member in members.members() {
        for chart in member.charts() {
            let Some(key) = chart.key() else {
                continue;
            };
            scores.records.push(B50Chart {
                key: key.clone(),
                source_song_id: member
                    .identity()
                    .canonical_song()
                    .cloned()
                    .ok_or("catalog member missing canonical identity")?,
                title: member.title().to_owned(),
                level: chart.level().to_owned(),
                constant: chart.constant(),
                achievements: Some(AchievementRate::from_decimal_str("100")?.into()),
                dx_score: None,
                rating: None,
                original_rating: None,
                grade: Some("sss".to_owned()),
                full_combo: Some(maimai_core::FullComboStatus::AllPerfectPlus),
                full_sync: Some(maimai_core::FullSyncStatus::FullSyncDeluxePlus),
                version: String::new(),
                is_current: false,
                fit_constant: None,
                fit_label: None,
            });
        }
    }
    let prepared = plate_progress::prepare(&snapshot, &scores, &spec, true)?;
    assert_eq!(prepared.listed_count, 0);
    assert_eq!(
        prepared.text,
        "已经没有剩余的的曲目了，恭喜alice完成「雪峰将」！"
    );
    Ok(())
}

#[test]
fn empty_real_plate_crosses_text_to_png_threshold() -> TestResult {
    let snapshot = snapshot()?;
    let scores = empty_scores(Lookup::Qq(maimai_core::QqId::new("123456")?));
    let prepared = plate_progress::prepare(
        &snapshot,
        &scores,
        &PlateSpec {
            version: PlateName::new("真")?,
            target: CompletionTarget::PlateGeneral,
            server: Some(PlateServer::Cn),
        },
        true,
    )?;
    assert!(prepared.listed_count > 10);
    assert!(prepared.text.starts_with("您的「真将」剩余进度如下："));
    Ok(())
}

#[test]
fn exact_eighty_multiple_has_no_empty_trailing_page() -> TestResult {
    let mut values = (0..80).collect::<Vec<_>>();
    let pages = values.len().div_ceil(80).max(1);
    retain_page(&mut values, 1, pages)?;
    assert_eq!(values.len(), 80);
    let error = retain_page(&mut values, 2, 1);
    assert!(error.is_err());
    Ok(())
}
