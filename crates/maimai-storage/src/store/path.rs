use std::{fs, io, path::Path};

use crate::StorageError;

pub(super) fn prepare_database_path(path: &Path) -> Result<(), StorageError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_existed = parent
        .try_exists()
        .map_err(|source| StorageError::PreparePath {
            path: parent.to_path_buf(),
            source,
        })?;
    fs::create_dir_all(parent).map_err(|source| StorageError::PreparePath {
        path: parent.to_path_buf(),
        source,
    })?;
    if !parent_existed {
        set_directory_permissions(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_database_file(path, &metadata),
        Err(source) if source.kind() == io::ErrorKind::NotFound => create_database_file(path),
        Err(source) => Err(path_error(path, source)),
    }
}

pub(super) fn harden_database_files(path: &Path) -> Result<(), StorageError> {
    harden_regular_file(path, true)?;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        harden_regular_file(Path::new(&sidecar), false)?;
    }
    Ok(())
}

fn validate_database_file(path: &Path, metadata: &fs::Metadata) -> Result<(), StorageError> {
    if metadata.file_type().is_symlink() {
        return Err(path_error(
            path,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "database path is a symbolic link",
            ),
        ));
    }
    if !metadata.is_file() {
        return Err(path_error(
            path,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "database path is not a regular file",
            ),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        StorageError::PreparePath {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> Result<(), StorageError> {
    Ok(())
}

#[cfg(unix)]
fn create_database_file(path: &Path) -> Result<(), StorageError> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(|source| path_error(path, source))?;
            validate_database_file(path, &metadata)
        }
        Err(source) => Err(path_error(path, source)),
    }
}

#[cfg(not(unix))]
fn create_database_file(path: &Path) -> Result<(), StorageError> {
    use std::fs::OpenOptions;

    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(|source| path_error(path, source))?;
            validate_database_file(path, &metadata)
        }
        Err(source) => Err(path_error(path, source)),
    }
}

#[cfg(unix)]
fn harden_regular_file(path: &Path, required: bool) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if !required && source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(path_error(path, source)),
    };
    validate_database_file(path, &metadata)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|source| path_error(path, source))
}

#[cfg(not(unix))]
fn harden_regular_file(path: &Path, required: bool) -> Result<(), StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_database_file(path, &metadata),
        Err(source) if !required && source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(path_error(path, source)),
    }
}

fn path_error(path: &Path, source: io::Error) -> StorageError {
    StorageError::PreparePath {
        path: path.to_path_buf(),
        source,
    }
}
