use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use super::CatalogRefreshJobStore;
use crate::StorageError;

#[derive(Debug)]
pub struct CatalogRefreshOwner {
    file: File,
    lock_path: PathBuf,
    held: AtomicBool,
}

impl CatalogRefreshOwner {
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }
}

#[derive(Debug)]
pub enum CatalogRefreshOwnerClaim {
    Acquired(CatalogRefreshOwner),
    AlreadyOwned { lock_path: PathBuf },
}

impl CatalogRefreshJobStore {
    pub fn claim_owner(&self) -> Result<CatalogRefreshOwnerClaim, StorageError> {
        let lock_path = lock_path(&self.database_path);
        let file = open_lock_file(&lock_path)?;
        match file.try_lock() {
            Ok(()) => Ok(CatalogRefreshOwnerClaim::Acquired(CatalogRefreshOwner {
                file,
                lock_path,
                held: AtomicBool::new(true),
            })),
            Err(TryLockError::WouldBlock) => {
                Ok(CatalogRefreshOwnerClaim::AlreadyOwned { lock_path })
            }
            Err(TryLockError::Error(source)) => Err(StorageError::PreparePath {
                path: lock_path,
                source,
            }),
        }
    }

    pub fn release_owner(&self, owner: &CatalogRefreshOwner) -> Result<bool, StorageError> {
        if !owner.held.swap(false, Ordering::AcqRel) {
            return Ok(false);
        }
        owner
            .file
            .unlock()
            .map(|()| true)
            .map_err(|source| StorageError::PreparePath {
                path: owner.lock_path.clone(),
                source,
            })
    }
}

fn lock_path(database_path: &Path) -> PathBuf {
    let mut value = database_path.as_os_str().to_os_string();
    value.push(".catalog-refresh.lock");
    PathBuf::from(value)
}

fn open_lock_file(path: &Path) -> Result<File, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_lock_file(path, &metadata)?,
        Err(source) if source.kind() == io::ErrorKind::NotFound => create_lock_file(path)?,
        Err(source) => return Err(path_error(path, source)),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|source| path_error(path, source))?;
    let opened_metadata = file.metadata().map_err(|source| path_error(path, source))?;
    validate_lock_file(path, &opened_metadata)?;
    let metadata = fs::symlink_metadata(path).map_err(|source| path_error(path, source))?;
    validate_lock_file(path, &metadata)?;
    harden_lock_file(&file, path)?;
    Ok(file)
}

#[cfg(unix)]
fn create_lock_file(path: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::OpenOptionsExt;

    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(path_error(path, source)),
    }
}

#[cfg(not(unix))]
fn create_lock_file(path: &Path) -> Result<(), StorageError> {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(path_error(path, source)),
    }
}

fn validate_lock_file(path: &Path, metadata: &fs::Metadata) -> Result<(), StorageError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(path_error(
            path,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "catalog refresh lock path is not a regular file",
            ),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn harden_lock_file(file: &File, path: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| path_error(path, source))
}

#[cfg(not(unix))]
fn harden_lock_file(_file: &File, _path: &Path) -> Result<(), StorageError> {
    Ok(())
}

fn path_error(path: &Path, source: io::Error) -> StorageError {
    StorageError::PreparePath {
        path: path.to_path_buf(),
        source,
    }
}
