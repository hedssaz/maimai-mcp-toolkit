use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;

use super::{AliasError, AliasKind};

pub(super) type AliasDocument = BTreeMap<String, Vec<String>>;

pub(super) fn read(
    configured_path: &Path,
    kind: AliasKind,
    missing_is_empty: bool,
) -> Result<(PathBuf, AliasDocument), AliasError> {
    let path = resolve_path(configured_path, missing_is_empty)?;
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound && missing_is_empty => {
            return Ok((path, AliasDocument::new()));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(AliasError::AliasFileMissing { kind });
        }
        Err(source) => {
            return Err(AliasError::Io {
                operation: "read",
                path,
                source,
            });
        }
    };
    let value =
        serde_json::from_str::<serde_json::Value>(&source).map_err(|source| AliasError::Json {
            path: path.clone(),
            source,
        })?;
    let document = serde_json::from_value::<AliasDocument>(value).map_err(|_| {
        AliasError::InvalidDocument {
            kind,
            path: path.clone(),
        }
    })?;
    Ok((path, document))
}

pub(super) fn write_atomic(path: &Path, document: &AliasDocument) -> Result<(), AliasError> {
    validate_target(path)?;
    let parent = path.parent().ok_or_else(|| AliasError::Io {
        operation: "resolve parent for",
        path: path.to_owned(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "alias path has no parent"),
    })?;
    validate_parent(parent)?;
    fs::create_dir_all(parent).map_err(|source| AliasError::Io {
        operation: "create parent for",
        path: path.to_owned(),
        source,
    })?;
    let permissions = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(AliasError::Io {
                operation: "read permissions for",
                path: path.to_owned(),
                source,
            });
        }
    };
    let mut temporary = NamedTempFile::new_in(parent).map_err(|source| AliasError::Io {
        operation: "create temporary file for",
        path: path.to_owned(),
        source,
    })?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), document).map_err(|source| {
        AliasError::Json {
            path: path.to_owned(),
            source,
        }
    })?;
    temporary
        .as_file_mut()
        .write_all(b"\n")
        .and_then(|()| temporary.as_file_mut().sync_all())
        .map_err(|source| AliasError::Io {
            operation: "write",
            path: path.to_owned(),
            source,
        })?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|source| AliasError::Io {
                operation: "preserve permissions for",
                path: path.to_owned(),
                source,
            })?;
    }
    temporary
        .persist(path)
        .map_err(|source| AliasError::Persist {
            path: path.to_owned(),
            source,
        })?;
    sync_parent(parent, path)
}

fn validate_target(path: &Path) -> Result<(), AliasError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AliasError::SymbolicLink {
            path: path.to_owned(),
        }),
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(AliasError::Io {
            operation: "validate",
            path: path.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "alias path is not a file"),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AliasError::Io {
            operation: "inspect",
            path: path.to_owned(),
            source,
        }),
    }
}

fn resolve_path(configured: &Path, may_not_exist: bool) -> Result<PathBuf, AliasError> {
    if let Some(parent) = configured.parent() {
        validate_parent(parent)?;
    }
    match fs::symlink_metadata(configured) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AliasError::SymbolicLink {
            path: configured.to_owned(),
        }),
        Ok(metadata) if metadata.is_file() => Ok(configured.to_owned()),
        Ok(_) => Err(AliasError::Io {
            operation: "validate",
            path: configured.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "alias path is not a file"),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound && may_not_exist => {
            Ok(configured.to_owned())
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(configured.to_owned()),
        Err(source) => Err(AliasError::Io {
            operation: "inspect",
            path: configured.to_owned(),
            source,
        }),
    }
}

fn validate_parent(parent: &Path) -> Result<(), AliasError> {
    let mut ancestors = parent
        .ancestors()
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    ancestors.reverse();
    for current in ancestors {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AliasError::SymbolicLink {
                    path: current.to_owned(),
                });
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(AliasError::Io {
                    operation: "validate parent component for",
                    path: current.to_owned(),
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "alias parent component is not a directory",
                    ),
                });
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(AliasError::Io {
                    operation: "inspect parent component for",
                    path: current.to_owned(),
                    source,
                });
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn sync_parent(parent: &Path, path: &Path) -> Result<(), AliasError> {
    let directory = fs::File::open(parent).map_err(|source| AliasError::Io {
        operation: "open parent directory for",
        path: path.to_owned(),
        source,
    })?;
    directory.sync_all().map_err(|source| AliasError::Io {
        operation: "sync parent directory for",
        path: path.to_owned(),
        source,
    })
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path, _path: &Path) -> Result<(), AliasError> {
    Ok(())
}
