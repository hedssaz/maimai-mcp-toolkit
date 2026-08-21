use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

use super::ImageOutputError;

pub(crate) fn secure_absolute(path: PathBuf) -> Result<PathBuf, ImageOutputError> {
    let absolute =
        std::path::absolute(&path).map_err(|source| ImageOutputError::io(path, source))?;
    if fs::symlink_metadata(&absolute).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ImageOutputError::UnsafePath { path: absolute });
    }
    let mut existing = absolute.clone();
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(&existing) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ImageOutputError::UnsafePath { path: existing });
            }
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = existing
                    .file_name()
                    .ok_or_else(|| ImageOutputError::UnsafePath {
                        path: absolute.clone(),
                    })?
                    .to_owned();
                missing.push(name);
                existing = existing
                    .parent()
                    .ok_or_else(|| ImageOutputError::UnsafePath {
                        path: absolute.clone(),
                    })?
                    .to_owned();
            }
            Err(error) => return Err(ImageOutputError::io(&existing, error)),
        }
    }
    let mut resolved = existing
        .canonicalize()
        .map_err(|source| ImageOutputError::io(&existing, source))?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

pub(crate) fn ensure_directory(path: &Path) -> Result<(), ImageOutputError> {
    reject_symlink_components(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(ImageOutputError::UnsafePath {
                path: path.to_owned(),
            });
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(ImageOutputError::io(path, error));
        }
        Err(_) => {}
    }
    let Some(parent) = path.parent() else {
        return Err(ImageOutputError::UnsafePath {
            path: path.to_owned(),
        });
    };
    if parent != path {
        ensure_directory(parent)?;
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => secure_directory_mode(path),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => ensure_directory(path),
        Err(error) => Err(ImageOutputError::io(path, error)),
    }
}

pub(crate) fn reject_non_regular(path: &Path) -> Result<bool, ImageOutputError> {
    reject_symlink_components(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(ImageOutputError::UnsafePath {
                path: path.to_owned(),
            })
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ImageOutputError::io(path, error)),
    }
}

pub(crate) fn reject_symlink_components(path: &Path) -> Result<(), ImageOutputError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink() || (current != path && !metadata.is_dir()) =>
            {
                return Err(ImageOutputError::UnsafePath { path: current });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(ImageOutputError::io(&current, error)),
        }
    }
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ImageOutputError> {
    let parent = path.parent().ok_or_else(|| ImageOutputError::UnsafePath {
        path: path.to_owned(),
    })?;
    ensure_directory(parent)?;
    reject_non_regular(path)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| ImageOutputError::io(parent, source))?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| ImageOutputError::io(temporary.path(), source))?;
    temporary
        .persist(path)
        .map_err(|error| ImageOutputError::io(path, error.error))?;
    secure_file_mode(path)?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| ImageOutputError::io(parent, source))?;
    Ok(())
}

pub(crate) fn atomic_write_unique(
    parent: &Path,
    filename: &str,
    bytes: &[u8],
) -> Result<PathBuf, ImageOutputError> {
    ensure_directory(parent)?;
    let (temporary_path, mut file) = create_temporary(parent, OsStr::new(filename))?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary_path);
        return Err(ImageOutputError::io(&temporary_path, error));
    }
    drop(file);
    for attempt in 0..1_024_u16 {
        let candidate = if attempt == 0 {
            parent.join(filename)
        } else {
            let stem = filename.strip_suffix(".png").unwrap_or(filename);
            parent.join(format!("{stem}-{attempt}.png"))
        };
        match fs::hard_link(&temporary_path, &candidate) {
            Ok(()) => {
                fs::remove_file(&temporary_path)
                    .map_err(|source| ImageOutputError::io(&temporary_path, source))?;
                secure_file_mode(&candidate)?;
                File::open(parent)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|source| ImageOutputError::io(parent, source))?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                let _ = fs::remove_file(&temporary_path);
                return Err(ImageOutputError::io(&candidate, error));
            }
        }
    }
    let _ = fs::remove_file(&temporary_path);
    Err(ImageOutputError::UnsafePath {
        path: parent.to_owned(),
    })
}

fn create_temporary(parent: &Path, name: &OsStr) -> Result<(PathBuf, File), ImageOutputError> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    for attempt in 0..128_u8 {
        let path = parent.join(format!(
            ".{}.tmp-{}-{nonce}-{attempt}",
            name.to_string_lossy(),
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ImageOutputError::io(&path, error)),
        }
    }
    Err(ImageOutputError::UnsafePath {
        path: parent.to_owned(),
    })
}

fn secure_directory_mode(path: &Path) -> Result<(), ImageOutputError> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|source| ImageOutputError::io(path, source))?;
    Ok(())
}

fn secure_file_mode(path: &Path) -> Result<(), ImageOutputError> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|source| ImageOutputError::io(path, source))?;
    Ok(())
}
