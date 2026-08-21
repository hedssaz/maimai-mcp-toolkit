use std::{
    collections::HashMap,
    error::Error,
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_providers::{CatalogSource, CatalogSourceClient, CatalogSourceConfig};
use tempfile::TempDir;

use super::{
    CatalogRefreshService, EnabledSources, OperationOutcome, RefreshErrorCode, RefreshRequest,
    ReloadSummary,
    fetch::{FetchFailure, FetchedBundle, FetchedDocument, FetchedStatus},
};

#[test]
fn audited_surface_allowlists_are_exact() {
    assert_eq!(EnabledSources::main().sources(), &CatalogSource::ALL);
    assert_eq!(
        EnabledSources::public().sources(),
        &[
            CatalogSource::Lxns,
            CatalogSource::DivingFish,
            CatalogSource::Yuzu,
            CatalogSource::ChartStats,
            CatalogSource::Plate,
        ]
    );
}

#[test]
fn plate_refresh_uses_explicit_non_default_catalog_path() -> Result<(), Box<dyn Error>> {
    let root = TempDir::new()?;
    let mut files = CatalogFiles::from_data_dir(fs::canonicalize(root.path())?);
    let custom = root.path().join("nested").join("custom-plate.json");
    files.maimaidxplate = Some(custom.clone());
    let paths = super::targets::TargetPaths::new(&files, &EnabledSources::public())?;
    assert_eq!(
        paths.path(CatalogSource::Plate, maimai_providers::SourceTarget::Plate)?,
        custom
    );
    Ok(())
}

#[tokio::test]
async fn check_only_uses_ttl_status_without_fetching() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let service = fixture.service(EnabledSources::main())?;
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let request = RefreshRequest::new(
        CatalogSource::ALL.to_vec(),
        1.0,
        false,
        true,
        Duration::from_secs(1),
    )?;
    let result = service
        .refresh_with(request, SystemTime::now(), move |_, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            async {
                Err(FetchFailure {
                    code: "UNEXPECTED".to_owned(),
                    message: "fetch should not run".to_owned(),
                })
            }
        })
        .await?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(result.due_sources().is_empty());
    assert_eq!(result.skipped_sources(), &CatalogSource::ALL);
    assert!(result.operations().is_empty());
    Ok(())
}

#[tokio::test]
async fn fetches_are_bounded_to_four_and_results_keep_request_order() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new().await?;
    let service = fixture.service(EnabledSources::main())?;
    let bundles = Arc::new(fixture.bundles()?);
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let request = RefreshRequest::new(
        CatalogSource::ALL.to_vec(),
        0.0,
        true,
        false,
        Duration::from_secs(2),
    )?;
    let result = service
        .refresh_with(request, SystemTime::now(), {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            move |source, _| {
                let active = Arc::clone(&active);
                let maximum = Arc::clone(&maximum);
                let bundle = bundles.get(&source).cloned();
                async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(current, Ordering::SeqCst);
                    let index = CatalogSource::ALL
                        .iter()
                        .position(|value| *value == source)
                        .unwrap_or_default();
                    tokio::time::sleep(Duration::from_millis(
                        u64::try_from(9 - index).unwrap_or_default() * 3,
                    ))
                    .await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    bundle.ok_or_else(|| FetchFailure {
                        code: "MISSING_FIXTURE".to_owned(),
                        message: "fixture bundle missing".to_owned(),
                    })
                }
            }
        })
        .await?;
    assert!(maximum.load(Ordering::SeqCst) <= 4);
    assert_eq!(result.refreshed_sources(), &CatalogSource::ALL);
    assert_eq!(
        result
            .operations()
            .iter()
            .map(|value| value.source())
            .collect::<Vec<_>>(),
        CatalogSource::ALL
    );
    assert_eq!(result.reload(), &ReloadSummary::Reloaded);
    Ok(())
}

#[tokio::test]
async fn one_failed_source_does_not_block_others() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let service = fixture.service(EnabledSources::main())?;
    let bundles = Arc::new(fixture.bundles()?);
    let requested = vec![
        CatalogSource::Plate,
        CatalogSource::DxData,
        CatalogSource::Location,
    ];
    let result = service
        .refresh_with(
            RefreshRequest::new(requested.clone(), 0.0, true, false, Duration::from_secs(1))?,
            SystemTime::now(),
            move |source, _| {
                let bundle = bundles.get(&source).cloned();
                async move {
                    if source == CatalogSource::DxData {
                        Err(FetchFailure {
                            code: "NETWORK".to_owned(),
                            message: "source request failed".to_owned(),
                        })
                    } else {
                        bundle.ok_or_else(|| FetchFailure {
                            code: "MISSING_FIXTURE".to_owned(),
                            message: "fixture bundle missing".to_owned(),
                        })
                    }
                }
            },
        )
        .await?;
    assert_eq!(
        result.refreshed_sources(),
        &[CatalogSource::Plate, CatalogSource::Location]
    );
    assert_eq!(result.failed_sources(), &[CatalogSource::DxData]);
    assert_eq!(
        result
            .operations()
            .iter()
            .map(|value| value.source())
            .collect::<Vec<_>>(),
        requested
    );
    assert_eq!(result.reload(), &ReloadSummary::Reloaded);
    Ok(())
}

#[tokio::test]
async fn incomplete_lxns_bundle_never_changes_either_target() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let service = fixture.service(EnabledSources::main())?;
    let before_songs = fs::read(&fixture.files.lxns_song_list)?;
    let before_aliases = fs::read(&fixture.files.lxns_alias_list)?;
    let result = service
        .refresh_with(
            RefreshRequest::new(
                vec![CatalogSource::Lxns],
                0.0,
                true,
                false,
                Duration::from_secs(1),
            )?,
            SystemTime::now(),
            |source, _| async move {
                Ok(FetchedBundle {
                    source,
                    status: FetchedStatus::Updated,
                    documents: vec![FetchedDocument {
                        target: maimai_providers::SourceTarget::LxnsSongList,
                        bytes: b"{\"songs\":[]}".to_vec(),
                    }],
                    etag: None,
                })
            },
        )
        .await?;
    assert_eq!(result.failed_sources(), &[CatalogSource::Lxns]);
    assert_eq!(fs::read(&fixture.files.lxns_song_list)?, before_songs);
    assert_eq!(fs::read(&fixture.files.lxns_alias_list)?, before_aliases);
    assert_eq!(result.reload(), &ReloadSummary::NotNeeded);
    Ok(())
}

#[tokio::test]
async fn second_lxns_replace_failure_rolls_back_first_file_and_keeps_snapshot()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let service = fixture.service(EnabledSources::main())?;
    let before_songs = fs::read(&fixture.files.lxns_song_list)?;
    let before_aliases = fs::read(&fixture.files.lxns_alias_list)?;
    let snapshot = fixture.store.snapshot();
    let failure = super::publish::publish_failing_at(
        &service.targets,
        FetchedBundle {
            source: CatalogSource::Lxns,
            status: FetchedStatus::Updated,
            documents: vec![
                FetchedDocument {
                    target: maimai_providers::SourceTarget::LxnsSongList,
                    bytes: br#"{"songs":[],"genres":[],"versions":[]}"#.to_vec(),
                },
                FetchedDocument {
                    target: maimai_providers::SourceTarget::LxnsAliasList,
                    bytes: br#"{"aliases":[{"song_id":1,"aliases":["new"]}]}"#.to_vec(),
                },
            ],
            etag: None,
        },
        SystemTime::now(),
        1,
    )
    .err()
    .ok_or("expected injected persist failure")?;
    assert_eq!(failure.code, "PUBLISH_FAILED");
    assert!(!failure.disk_updated);
    assert_eq!(fs::read(&fixture.files.lxns_song_list)?, before_songs);
    assert_eq!(fs::read(&fixture.files.lxns_alias_list)?, before_aliases);
    assert!(Arc::ptr_eq(&fixture.store.snapshot(), &snapshot));
    Ok(())
}

#[tokio::test]
async fn not_modified_requires_target_and_updates_freshness() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let service = fixture.service(EnabledSources::main())?;
    let before = fs::metadata(&fixture.files.diving_fish_song_list)?.modified()?;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let result = service
        .refresh_with(
            RefreshRequest::new(
                vec![CatalogSource::DivingFish],
                0.0,
                true,
                false,
                Duration::from_secs(1),
            )?,
            SystemTime::now(),
            not_modified,
        )
        .await?;
    let after = fs::metadata(&fixture.files.diving_fish_song_list)?.modified()?;
    assert!(after > before);
    assert_eq!(
        result.operations()[0].outcome(),
        OperationOutcome::NotModified
    );

    fs::remove_file(&fixture.files.diving_fish_song_list)?;
    let missing = service
        .refresh_with(
            RefreshRequest::new(
                vec![CatalogSource::DivingFish],
                0.0,
                true,
                false,
                Duration::from_secs(1),
            )?,
            SystemTime::now(),
            not_modified,
        )
        .await?;
    assert_eq!(missing.failed_sources(), &[CatalogSource::DivingFish]);
    assert_eq!(
        missing.operations()[0].error_code(),
        Some("NOT_MODIFIED_WITHOUT_TARGET")
    );
    Ok(())
}

#[tokio::test]
async fn reload_failure_keeps_old_snapshot_and_reports_pending_disk_state()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new().await?;
    let service = fixture.service(EnabledSources::main())?;
    let old_snapshot = fixture.store.snapshot();
    let result = service
        .refresh_with(
            RefreshRequest::new(
                vec![CatalogSource::ChartStats],
                0.0,
                true,
                false,
                Duration::from_secs(1),
            )?,
            SystemTime::now(),
            |source, _| async move {
                Ok(FetchedBundle {
                    source,
                    status: FetchedStatus::Updated,
                    documents: vec![FetchedDocument {
                        target: maimai_providers::SourceTarget::DivingFishChartStats,
                        bytes: br#"{"charts":[]}"#.to_vec(),
                    }],
                    etag: None,
                })
            },
        )
        .await?;
    assert!(matches!(result.reload(), ReloadSummary::Failed { .. }));
    assert!(result.refreshed_sources().is_empty());
    assert_eq!(result.failed_sources(), &[CatalogSource::ChartStats]);
    assert_eq!(
        result.operations()[0].outcome(),
        OperationOutcome::DiskUpdatedPendingReload
    );
    assert_eq!(
        fixture.store.snapshot().songs().len(),
        old_snapshot.songs().len()
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symbolic_link_target_is_rejected_before_fetch() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new().await?;
    let service = fixture.service(EnabledSources::main())?;
    let plate = fixture.root.path().join("maimaidxplate.json");
    let outside = fixture.root.path().join("outside.json");
    fs::write(&outside, "{}")?;
    fs::remove_file(&plate)?;
    symlink(&outside, &plate)?;
    let error = service
        .refresh(RefreshRequest::new(
            vec![CatalogSource::Plate],
            0.0,
            false,
            true,
            Duration::from_secs(1),
        )?)
        .await
        .err()
        .ok_or("expected symbolic link rejection")?;
    assert_eq!(error.code(), RefreshErrorCode::InvalidTarget);
    assert_eq!(fs::read_to_string(outside)?, "{}");
    Ok(())
}

async fn not_modified(
    source: CatalogSource,
    _etag: Option<maimai_providers::EntityTag>,
) -> Result<FetchedBundle, FetchFailure> {
    Ok(FetchedBundle {
        source,
        status: FetchedStatus::NotModified,
        documents: Vec::new(),
        etag: None,
    })
}

struct Fixture {
    root: TempDir,
    files: CatalogFiles,
    store: Arc<CatalogStore>,
}

impl Fixture {
    async fn new() -> Result<Self, Box<dyn Error>> {
        let root = TempDir::new()?;
        let mut files = CatalogFiles::from_data_dir(fs::canonicalize(root.path())?);
        files.official_music_data = None;
        files.legacy_aliases_csv = None;
        files.artist_aliases = None;
        files.charter_aliases = None;
        files.traditional_to_simplified = None;
        write_fixture(&files)?;
        fs::write(root.path().join("maimaidxplate.json"), "{}")?;
        fs::write(
            root.path().join("sega_maidx_locations.json"),
            r#"{"locations":[{"id":1}]}"#,
        )?;
        let store = Arc::new(CatalogStore::load(files.clone()).await?);
        Ok(Self { root, files, store })
    }

    fn service(&self, enabled: EnabledSources) -> Result<CatalogRefreshService, Box<dyn Error>> {
        let client = Arc::new(CatalogSourceClient::new(CatalogSourceConfig::default())?);
        Ok(CatalogRefreshService::new(
            client,
            Arc::clone(&self.store),
            enabled,
        )?)
    }

    fn bundles(&self) -> Result<HashMap<CatalogSource, FetchedBundle>, Box<dyn Error>> {
        CatalogSource::ALL
            .into_iter()
            .map(|source| {
                let documents = source
                    .targets()
                    .iter()
                    .map(|target| {
                        let path = match target {
                            maimai_providers::SourceTarget::Plate
                            | maimai_providers::SourceTarget::Location => {
                                self.root.path().join(target.file_name())
                            }
                            _ => target_path(&self.files, *target)?,
                        };
                        Ok(FetchedDocument {
                            target: *target,
                            bytes: fs::read(path)?,
                        })
                    })
                    .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
                Ok((
                    source,
                    FetchedBundle {
                        source,
                        status: FetchedStatus::Updated,
                        documents,
                        etag: None,
                    },
                ))
            })
            .collect()
    }
}

fn target_path(
    files: &CatalogFiles,
    target: maimai_providers::SourceTarget,
) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let path = match target {
        maimai_providers::SourceTarget::LxnsSongList => files.lxns_song_list.clone(),
        maimai_providers::SourceTarget::LxnsAliasList => files.lxns_alias_list.clone(),
        maimai_providers::SourceTarget::DivingFishSongList => files.diving_fish_song_list.clone(),
        maimai_providers::SourceTarget::YuzuAliasList => files.yuzu_alias_list.clone(),
        maimai_providers::SourceTarget::DxData => {
            files.dxdata.clone().ok_or("dxdata path missing")?
        }
        maimai_providers::SourceTarget::DivingFishChartStats => {
            files.chart_stats.clone().ok_or("stats path missing")?
        }
        maimai_providers::SourceTarget::DxRatingAliases => files
            .dxrating_aliases
            .clone()
            .ok_or("aliases path missing")?,
        maimai_providers::SourceTarget::DxRatingTags => {
            files.tags.clone().ok_or("tags path missing")?
        }
        maimai_providers::SourceTarget::Plate | maimai_providers::SourceTarget::Location => {
            return Err("external target requested through catalog files".into());
        }
    };
    Ok(path)
}

fn write_fixture(files: &CatalogFiles) -> Result<(), Box<dyn Error>> {
    fs::write(
        &files.lxns_song_list,
        r#"{"songs":[{"id":1,"title":"Alpha","artist":"Alice","genre":"game","bpm":160,"version":10000,"difficulties":{"dx":[{"difficulty":3,"level":"13+","level_value":13.5,"note_designer":"Carol","notes":{"tap":1}}]}}],"genres":[],"versions":[]}"#,
    )?;
    fs::write(&files.diving_fish_song_list, "[]")?;
    fs::write(&files.lxns_alias_list, r#"{"aliases":[]}"#)?;
    fs::write(&files.yuzu_alias_list, r#"{"content":[]}"#)?;
    fs::write(&files.custom_aliases, "{}")?;
    fs::write(&files.pinyin_aliases, r#"{"aliases":[]}"#)?;
    fs::write(&files.simplified_to_traditional, "{}")?;
    fs::write(
        files.dxdata.as_ref().ok_or("dxdata path missing")?,
        r#"{"songs":[],"versions":[]}"#,
    )?;
    fs::write(
        files.chart_stats.as_ref().ok_or("stats path missing")?,
        r#"{"charts":{},"diff_data":{}}"#,
    )?;
    fs::write(
        files.tags.as_ref().ok_or("tags path missing")?,
        r#"{"tags":[],"tagGroups":[],"tagSongs":[]}"#,
    )?;
    fs::write(
        files
            .dxrating_aliases
            .as_ref()
            .ok_or("aliases path missing")?,
        "[]",
    )?;
    Ok(())
}
