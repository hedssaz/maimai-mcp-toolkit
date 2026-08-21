use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use maimai_catalog::CatalogFiles;
use maimai_providers::{CatalogSource, EntityTag, SourceTarget};

use super::{
    error::RefreshError,
    model::{EnabledSources, SourceStatus, TargetStatus},
};

#[derive(Clone, Debug)]
pub(crate) struct TargetPaths {
    paths: HashMap<SourceTarget, PathBuf>,
    etag: PathBuf,
}

impl TargetPaths {
    pub(crate) fn new(
        files: &CatalogFiles,
        enabled: &EnabledSources,
    ) -> Result<Self, RefreshError> {
        let data_dir = files
            .lxns_song_list
            .parent()
            .ok_or(RefreshError::InvalidTarget {
                catalog_source: CatalogSource::Lxns.name(),
                target: SourceTarget::LxnsSongList.file_name(),
            })?
            .to_owned();
        let mut paths = HashMap::new();
        for source in enabled.sources() {
            for target in source.targets() {
                let path = configured_path(files, &data_dir, *source, *target)?;
                paths.insert(*target, path);
            }
        }
        Ok(Self {
            paths,
            etag: data_dir.join(".divingfish_etag"),
        })
    }

    pub(crate) fn path(
        &self,
        source: CatalogSource,
        target: SourceTarget,
    ) -> Result<&Path, RefreshError> {
        self.paths
            .get(&target)
            .map(PathBuf::as_path)
            .ok_or(RefreshError::InvalidTarget {
                catalog_source: source.name(),
                target: target.file_name(),
            })
    }

    pub(crate) fn etag_path(&self) -> &Path {
        &self.etag
    }
}

pub(crate) fn statuses(
    paths: &TargetPaths,
    sources: &[CatalogSource],
    ttl_days: f64,
    now: SystemTime,
) -> Result<Vec<SourceStatus>, RefreshError> {
    let ttl =
        Duration::try_from_secs_f64(ttl_days * 86_400.0).map_err(|_| RefreshError::InvalidTtl)?;
    sources
        .iter()
        .map(|source| status(paths, *source, ttl_days, ttl, now))
        .collect()
}

fn status(
    paths: &TargetPaths,
    source: CatalogSource,
    ttl_days: f64,
    ttl: Duration,
    now: SystemTime,
) -> Result<SourceStatus, RefreshError> {
    let targets = source
        .targets()
        .iter()
        .map(|target| {
            let path = paths.path(source, *target)?;
            let modified = inspect_target(path, source, *target)?;
            let age = modified.map(|value| now.duration_since(value).unwrap_or(Duration::ZERO));
            Ok(TargetStatus {
                target: *target,
                exists: modified.is_some(),
                modified,
                age,
                expired: age.is_none_or(|value| value >= ttl),
            })
        })
        .collect::<Result<Vec<_>, RefreshError>>()?;
    let oldest_modified = targets.iter().filter_map(TargetStatus::modified).min();
    let age = oldest_modified.map(|value| now.duration_since(value).unwrap_or(Duration::ZERO));
    let expired = targets.iter().any(TargetStatus::expired);
    Ok(SourceStatus {
        source,
        targets,
        oldest_modified,
        age,
        ttl_days,
        expired,
    })
}

pub(crate) fn inspect_target(
    path: &Path,
    source: CatalogSource,
    target: SourceTarget,
) -> Result<Option<SystemTime>, RefreshError> {
    let parent = path.parent().ok_or(RefreshError::InvalidTarget {
        catalog_source: source.name(),
        target: target.file_name(),
    })?;
    validate_parent(parent, source, target)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(RefreshError::InvalidTarget {
                catalog_source: source.name(),
                target: target.file_name(),
            })
        }
        Ok(metadata) => metadata
            .modified()
            .map(Some)
            .map_err(|error| RefreshError::target(source, target, "read metadata", error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(RefreshError::target(source, target, "inspect", error)),
    }
}

pub(crate) fn read_etag(paths: &TargetPaths) -> Result<Option<EntityTag>, RefreshError> {
    validate_logical_path(paths.etag_path(), CatalogSource::DivingFish)?;
    let source = match fs::read_to_string(paths.etag_path()) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(RefreshError::logical(
                CatalogSource::DivingFish,
                "read entity tag",
                error,
            ));
        }
    };
    EntityTag::parse(source.trim())
        .map(Some)
        .map_err(|_| RefreshError::InvalidEntityTag)
}

pub(crate) fn validate_logical_path(
    path: &Path,
    source: CatalogSource,
) -> Result<(), RefreshError> {
    let parent = path.parent().ok_or(RefreshError::InvalidTarget {
        catalog_source: source.name(),
        target: "source metadata",
    })?;
    validate_parent_logical(parent, source)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(RefreshError::InvalidTarget {
                catalog_source: source.name(),
                target: "source metadata",
            })
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RefreshError::logical(source, "inspect", error)),
    }
}

fn validate_parent(
    parent: &Path,
    source: CatalogSource,
    target: SourceTarget,
) -> Result<(), RefreshError> {
    validate_ancestors(parent).map_err(|error| match error {
        ParentError::Invalid => RefreshError::InvalidTarget {
            catalog_source: source.name(),
            target: target.file_name(),
        },
        ParentError::Io(error) => RefreshError::target(source, target, "inspect parent", error),
    })
}

fn validate_parent_logical(parent: &Path, source: CatalogSource) -> Result<(), RefreshError> {
    validate_ancestors(parent).map_err(|error| match error {
        ParentError::Invalid => RefreshError::InvalidTarget {
            catalog_source: source.name(),
            target: "source metadata",
        },
        ParentError::Io(error) => RefreshError::logical(source, "inspect parent", error),
    })
}

enum ParentError {
    Invalid,
    Io(std::io::Error),
}

fn validate_ancestors(parent: &Path) -> Result<(), ParentError> {
    let mut ancestors = parent.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for ancestor in ancestors {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(ParentError::Invalid);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ParentError::Invalid);
            }
            Err(error) => return Err(ParentError::Io(error)),
        }
    }
    Ok(())
}

fn configured_path(
    files: &CatalogFiles,
    data_dir: &Path,
    source: CatalogSource,
    target: SourceTarget,
) -> Result<PathBuf, RefreshError> {
    let path = match target {
        SourceTarget::LxnsSongList => Some(&files.lxns_song_list),
        SourceTarget::LxnsAliasList => Some(&files.lxns_alias_list),
        SourceTarget::DivingFishSongList => Some(&files.diving_fish_song_list),
        SourceTarget::YuzuAliasList => Some(&files.yuzu_alias_list),
        SourceTarget::DxData => files.dxdata.as_ref(),
        SourceTarget::DivingFishChartStats => files.chart_stats.as_ref(),
        SourceTarget::DxRatingAliases => files.dxrating_aliases.as_ref(),
        SourceTarget::DxRatingTags => files.tags.as_ref(),
        SourceTarget::Plate => files.maimaidxplate.as_ref(),
        SourceTarget::Location => None,
    };
    if let Some(path) = path {
        return Ok(path.clone());
    }
    if target == SourceTarget::Location {
        return Ok(data_dir.join(target.file_name()));
    }
    Err(RefreshError::InvalidTarget {
        catalog_source: source.name(),
        target: target.file_name(),
    })
}
