use std::{error::Error, path::Path};

use maimai_catalog::CatalogFiles;
use maimai_core::SongIdValue;

use super::{KnownMusicMetadata, MusicInfoChartType, MusicInfoRequest, resolve::resolve};

#[test]
fn real_catalog_keeps_exact_identity_type_inference_and_dual_order() -> Result<(), Box<dyn Error>> {
    let snapshot = real_catalog()?;
    let dual = resolve(&snapshot, &request(None, Some("相信彩虹"), None)?)?;
    assert_eq!(
        dual.iter()
            .map(|value| value.chart_type)
            .collect::<Vec<_>>(),
        [
            Some(MusicInfoChartType::Standard),
            Some(MusicInfoChartType::Deluxe)
        ]
    );
    assert_eq!(dual[0].music_id, "835");
    assert_eq!(dual[1].music_id, "10835");

    let inferred = resolve(&snapshot, &request(None, Some("dx相信彩虹"), None)?)?;
    assert_eq!(inferred.len(), 1);
    assert_eq!(inferred[0].chart_type, Some(MusicInfoChartType::Deluxe));
    assert_eq!(inferred[0].music_id, "10835");

    let exact_suffix = resolve(&snapshot, &request(None, Some("MEGATON BLAST"), None)?)?;
    assert_eq!(exact_suffix.len(), 1);
    assert_eq!(exact_suffix[0].chart_type, Some(MusicInfoChartType::Deluxe));
    Ok(())
}

#[test]
fn explicit_ids_normalize_both_directions_and_cover_only_image_needs_no_id()
-> Result<(), Box<dyn Error>> {
    let snapshot = real_catalog()?;
    let deluxe = resolve(
        &snapshot,
        &request(
            Some(SongIdValue::Numeric(835)),
            None,
            Some(MusicInfoChartType::Deluxe),
        )?,
    )?;
    assert_eq!(deluxe[0].music_id, "10835");
    let standard = resolve(
        &snapshot,
        &request(
            Some(SongIdValue::Numeric(10_835)),
            None,
            Some(MusicInfoChartType::Standard),
        )?,
    )?;
    assert_eq!(standard[0].music_id, "835");

    let cover_only = MusicInfoRequest::new(
        None,
        None,
        None,
        Some("local-cover".to_owned()),
        KnownMusicMetadata::default(),
        None,
    )?;
    let prepared = resolve(&snapshot, &cover_only)?;
    assert!(prepared[0].view.song_id.is_none());
    assert!(prepared[0].view.require_cover);
    Ok(())
}

#[test]
fn real_catalog_preserves_whitespace_title_and_reports_ambiguous_matches()
-> Result<(), Box<dyn Error>> {
    let snapshot = real_catalog()?;
    let whitespace = resolve(
        &snapshot,
        &request(Some(SongIdValue::Numeric(11_422)), None, None)?,
    )?;
    assert_eq!(whitespace[0].title, "　");

    let error = resolve(&snapshot, &request(None, Some("Link"), None)?)
        .err()
        .ok_or("Link should be ambiguous")?;
    assert!(
        error
            .to_string()
            .starts_with("匹配到多个曲目，请指定更精确的曲名或 ID:\n")
    );
    Ok(())
}

fn request(
    id: Option<SongIdValue>,
    query: Option<&str>,
    chart_type: Option<MusicInfoChartType>,
) -> Result<MusicInfoRequest, super::MusicInfoError> {
    MusicInfoRequest::new(
        id,
        query.map(str::to_owned),
        chart_type,
        None,
        KnownMusicMetadata::default(),
        None,
    )
}

fn real_catalog() -> Result<maimai_catalog::CatalogSnapshot, maimai_catalog::CatalogError> {
    CatalogFiles::from_data_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).load()
}
