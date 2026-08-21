use std::{
    fs::{self, FileTimes, Permissions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use maimai_providers::{CatalogSource, EntityTag};
use tempfile::NamedTempFile;

use super::{
    fetch::{FetchedBundle, FetchedDocument, FetchedStatus},
    targets::{TargetPaths, inspect_target, validate_logical_path},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublishOutcome {
    Updated,
    NotModified,
}

#[derive(Clone, Debug)]
pub(crate) struct PublishFailure {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
    pub(crate) disk_updated: bool,
}

pub(crate) fn publish(
    paths: &TargetPaths,
    bundle: FetchedBundle,
    now: SystemTime,
) -> Result<PublishOutcome, PublishFailure> {
    publish_inner(paths, bundle, now, None)
}

fn publish_inner(
    paths: &TargetPaths,
    bundle: FetchedBundle,
    now: SystemTime,
    fail_at: Option<usize>,
) -> Result<PublishOutcome, PublishFailure> {
    match bundle.status {
        FetchedStatus::Updated => publish_updated(paths, bundle, fail_at),
        FetchedStatus::NotModified => publish_not_modified(paths, bundle, now, fail_at),
    }
}

#[cfg(test)]
pub(crate) fn publish_failing_at(
    paths: &TargetPaths,
    bundle: FetchedBundle,
    now: SystemTime,
    fail_at: usize,
) -> Result<PublishOutcome, PublishFailure> {
    publish_inner(paths, bundle, now, Some(fail_at))
}

fn publish_updated(
    paths: &TargetPaths,
    bundle: FetchedBundle,
    fail_at: Option<usize>,
) -> Result<PublishOutcome, PublishFailure> {
    validate_documents(&bundle)?;
    let mut prepared =
        Vec::with_capacity(bundle.documents.len() + usize::from(bundle.etag.is_some()));
    for document in bundle.documents {
        prepared.push(prepare_document(paths, bundle.source, document)?);
    }
    if bundle.source == CatalogSource::DivingFish
        && let Some(etag) = bundle.etag.as_ref()
    {
        prepared.push(prepare_etag(paths, etag)?);
    }
    persist_all(prepared, fail_at)?;
    Ok(PublishOutcome::Updated)
}

fn publish_not_modified(
    paths: &TargetPaths,
    bundle: FetchedBundle,
    now: SystemTime,
    fail_at: Option<usize>,
) -> Result<PublishOutcome, PublishFailure> {
    if bundle.source != CatalogSource::DivingFish || !bundle.documents.is_empty() {
        return Err(invalid_bundle());
    }
    for target in bundle.source.targets() {
        let path = paths
            .path(bundle.source, *target)
            .map_err(|_| invalid_target())?;
        let exists = inspect_target(path, bundle.source, *target)
            .map_err(|_| invalid_target())?
            .is_some();
        if !exists {
            return Err(PublishFailure {
                code: "NOT_MODIFIED_WITHOUT_TARGET",
                message: "source returned not-modified but the local target is missing",
                disk_updated: false,
            });
        }
    }
    let prepared = bundle
        .etag
        .as_ref()
        .map(|etag| prepare_etag(paths, etag))
        .transpose()?;
    let mut disk_updated = false;
    if let Some(file) = prepared {
        persist_all(vec![file], fail_at)?;
        disk_updated = true;
    }
    for target in bundle.source.targets() {
        let path = paths
            .path(bundle.source, *target)
            .map_err(|_| invalid_target())?;
        let file = fs::File::open(path).map_err(|_| PublishFailure {
            code: "TOUCH_FAILED",
            message: "failed to update source freshness timestamp",
            disk_updated,
        })?;
        file.set_times(FileTimes::new().set_modified(now))
            .map_err(|_| PublishFailure {
                code: "TOUCH_FAILED",
                message: "failed to update source freshness timestamp",
                disk_updated,
            })?;
        disk_updated = true;
    }
    Ok(PublishOutcome::NotModified)
}

fn validate_documents(bundle: &FetchedBundle) -> Result<(), PublishFailure> {
    let expected = bundle.source.targets();
    if bundle.documents.len() != expected.len() {
        return Err(invalid_bundle());
    }
    let mut seen = Vec::with_capacity(bundle.documents.len());
    for document in &bundle.documents {
        if !expected.contains(&document.target) || seen.contains(&document.target) {
            return Err(invalid_bundle());
        }
        seen.push(document.target);
    }
    Ok(())
}

fn prepare_document(
    paths: &TargetPaths,
    source: CatalogSource,
    document: FetchedDocument,
) -> Result<PreparedFile, PublishFailure> {
    let path = paths
        .path(source, document.target)
        .map_err(|_| invalid_target())?;
    inspect_target(path, source, document.target).map_err(|_| invalid_target())?;
    prepare_file(path, document.target.file_name(), &document.bytes)
}

fn prepare_etag(paths: &TargetPaths, etag: &EntityTag) -> Result<PreparedFile, PublishFailure> {
    validate_logical_path(paths.etag_path(), CatalogSource::DivingFish)
        .map_err(|_| invalid_target())?;
    let header = etag.as_header_value();
    let sidecar = header
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(header);
    prepare_file(paths.etag_path(), ".divingfish_etag", sidecar.as_bytes())
}

struct PreparedFile {
    target: PathBuf,
    logical_target: &'static str,
    temporary: NamedTempFile,
}

fn prepare_file(
    target: &Path,
    logical_target: &'static str,
    bytes: &[u8],
) -> Result<PreparedFile, PublishFailure> {
    let parent = target.parent().ok_or_else(invalid_target)?;
    let permissions = permissions(target)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|_| prepare_failed())?;
    temporary
        .as_file_mut()
        .write_all(bytes)
        .and_then(|()| temporary.as_file_mut().flush())
        .and_then(|()| temporary.as_file_mut().sync_all())
        .map_err(|_| prepare_failed())?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|_| prepare_failed())?;
    }
    Ok(PreparedFile {
        target: target.to_owned(),
        logical_target,
        temporary,
    })
}

fn permissions(path: &Path) -> Result<Option<Permissions>, PublishFailure> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(invalid_target())
        }
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(prepare_failed()),
    }
}

fn persist_all(prepared: Vec<PreparedFile>, fail_at: Option<usize>) -> Result<(), PublishFailure> {
    let mut backups = prepared.iter().map(backup).collect::<Result<Vec<_>, _>>()?;
    let mut changed = 0_usize;
    let mut parents = Vec::new();
    for (index, prepared) in prepared.into_iter().enumerate() {
        let parent = prepared
            .target
            .parent()
            .map(Path::to_owned)
            .ok_or_else(invalid_target)?;
        let persist_failed =
            fail_at == Some(index) || prepared.temporary.persist(&prepared.target).is_err();
        if persist_failed {
            return rollback(&mut backups, changed, &parents).map_or_else(
                |_| {
                    Err(PublishFailure {
                        code: "ROLLBACK_FAILED",
                        message: "source publication failed and the previous files could not be fully restored",
                        disk_updated: changed > 0,
                    })
                },
                |()| {
                    Err(PublishFailure {
                        code: "PUBLISH_FAILED",
                        message: prepared.logical_target,
                        disk_updated: false,
                    })
                },
            );
        }
        changed += 1;
        if !parents.contains(&parent) {
            parents.push(parent);
        }
    }
    for parent in parents {
        if sync_parent(&parent).is_err() {
            return match rollback(&mut backups, changed, &[]) {
                Ok(()) => Err(PublishFailure {
                    code: "PUBLISH_SYNC_FAILED",
                    message: "source directory sync failed; previous source files were restored",
                    disk_updated: false,
                }),
                Err(()) => Err(PublishFailure {
                    code: "ROLLBACK_FAILED",
                    message: "source directory sync failed and previous files could not be fully restored",
                    disk_updated: changed > 0,
                }),
            };
        }
    }
    Ok(())
}

struct BackupFile {
    target: PathBuf,
    previous: Option<NamedTempFile>,
}

fn backup(prepared: &PreparedFile) -> Result<BackupFile, PublishFailure> {
    let previous = match fs::symlink_metadata(&prepared.target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(invalid_target());
        }
        Ok(_) => {
            let bytes = fs::read(&prepared.target).map_err(|_| prepare_failed())?;
            Some(prepare_file(&prepared.target, prepared.logical_target, &bytes)?.temporary)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return Err(prepare_failed()),
    };
    Ok(BackupFile {
        target: prepared.target.clone(),
        previous,
    })
}

fn rollback(
    backups: &mut [BackupFile],
    changed: usize,
    known_parents: &[PathBuf],
) -> Result<(), ()> {
    let mut parents = known_parents.to_vec();
    for backup in backups.iter_mut().take(changed).rev() {
        let parent = backup.target.parent().map(Path::to_owned).ok_or(())?;
        if let Some(previous) = backup.previous.take() {
            previous.persist(&backup.target).map_err(|_| ())?;
        } else {
            match fs::remove_file(&backup.target) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(()),
            }
        }
        if !parents.contains(&parent) {
            parents.push(parent);
        }
    }
    for parent in parents {
        sync_parent(&parent).map_err(|_| ())?;
    }
    Ok(())
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), io::Error> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), io::Error> {
    Ok(())
}

const fn invalid_bundle() -> PublishFailure {
    PublishFailure {
        code: "INVALID_SOURCE_BUNDLE",
        message: "provider returned an incomplete or mismatched source bundle",
        disk_updated: false,
    }
}

const fn invalid_target() -> PublishFailure {
    PublishFailure {
        code: "INVALID_TARGET",
        message: "source target is a symbolic link, non-file, or invalid path",
        disk_updated: false,
    }
}

const fn prepare_failed() -> PublishFailure {
    PublishFailure {
        code: "PREPARE_FAILED",
        message: "source bundle could not be prepared for atomic replacement",
        disk_updated: false,
    }
}
