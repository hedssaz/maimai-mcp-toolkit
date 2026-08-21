use std::{
    cmp::Reverse,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use time::{Duration, OffsetDateTime, UtcOffset};

use super::{
    ImageOutputError,
    atomic_file::{
        atomic_write_unique, ensure_directory, reject_symlink_components, secure_absolute,
    },
};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_RETAINED_FILES: usize = 10_000;
const MAX_TTL_DAYS: i64 = 365;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageOutputPolicy {
    ttl: Duration,
    max_files: usize,
}

impl ImageOutputPolicy {
    pub fn new(ttl: Duration, max_files: usize) -> Result<Self, ImageOutputError> {
        if !ttl.is_positive() || ttl > Duration::days(MAX_TTL_DAYS) {
            return Err(ImageOutputError::InvalidPolicy {
                message: "ttl must be between one nanosecond and 365 days",
            });
        }
        if max_files == 0 || max_files > MAX_RETAINED_FILES {
            return Err(ImageOutputError::InvalidPolicy {
                message: "max_files must be between 1 and 10000",
            });
        }
        Ok(Self { ttl, max_files })
    }

    pub const fn standard() -> Self {
        Self {
            ttl: Duration::days(7),
            max_files: 200,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImageOutputStore {
    root: PathBuf,
    policy: ImageOutputPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedImage {
    pub path: PathBuf,
    pub removed_files: usize,
}

impl ImageOutputStore {
    pub fn new(
        root: impl Into<PathBuf>,
        policy: ImageOutputPolicy,
    ) -> Result<Self, ImageOutputError> {
        let root = secure_absolute(root.into())?;
        reject_symlink_components(&root)?;
        if let Ok(metadata) = fs::symlink_metadata(&root)
            && (metadata.file_type().is_symlink() || !metadata.is_dir())
        {
            return Err(ImageOutputError::UnsafePath { path: root });
        }
        Ok(Self { root, policy })
    }

    pub fn at_root(&self, root: impl Into<PathBuf>) -> Result<Self, ImageOutputError> {
        Self::new(root, self.policy)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Materializes the already validated root without writing an image.
    pub fn ensure_root(&self) -> Result<(), ImageOutputError> {
        ensure_directory(&self.root)
    }

    pub fn save_png(
        &self,
        filename_stem: &str,
        png: &[u8],
        now: OffsetDateTime,
    ) -> Result<SavedImage, ImageOutputError> {
        if !png.starts_with(PNG_SIGNATURE) {
            return Err(ImageOutputError::InvalidPolicy {
                message: "rendered bytes are not a PNG",
            });
        }
        ensure_directory(&self.root)?;
        let filename = format!(
            "{}_{}.png",
            safe_stem(filename_stem),
            compact_timestamp(now)
        );
        let path = atomic_write_unique(&self.root, &filename, png)?;
        let removed_files = self.cleanup(now)?;
        Ok(SavedImage {
            path,
            removed_files,
        })
    }

    pub fn cleanup(&self, now: OffsetDateTime) -> Result<usize, ImageOutputError> {
        let mut files = self.png_files()?;
        let cutoff = now - self.policy.ttl;
        let mut removed = 0;
        for file in &files {
            if OffsetDateTime::from(file.modified) <= cutoff && remove_file(&file.path)? {
                removed += 1;
            }
        }
        files.retain(|file| file.path.exists());
        files.sort_by_key(|file| Reverse(file.modified));
        for file in files.iter().skip(self.policy.max_files) {
            if remove_file(&file.path)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn png_files(&self) -> Result<Vec<OutputFile>, ImageOutputError> {
        let entries =
            fs::read_dir(&self.root).map_err(|source| ImageOutputError::io(&self.root, source))?;
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| ImageOutputError::io(&self.root, source))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("png") {
                continue;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(ImageOutputError::io(&path, error)),
            };
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                continue;
            }
            let modified = metadata
                .modified()
                .map_err(|source| ImageOutputError::io(&path, source))?;
            files.push(OutputFile { path, modified });
        }
        Ok(files)
    }
}

struct OutputFile {
    path: PathBuf,
    modified: SystemTime,
}

fn safe_stem(value: &str) -> String {
    let result = value
        .chars()
        .take(60)
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if result.is_empty() {
        "maimai".to_owned()
    } else {
        result
    }
}

fn compact_timestamp(value: OffsetDateTime) -> String {
    let value = value.to_offset(UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn remove_file(path: &Path) -> Result<bool, ImageOutputError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ImageOutputError::io(path, error)),
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fs};

    #[cfg(unix)]
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    #[test]
    fn ensure_root_creates_missing_directory_and_is_idempotent() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("nested/output");
        let store = ImageOutputStore::new(&root, ImageOutputPolicy::standard())?;
        assert!(!root.exists());
        store.ensure_root()?;
        store.ensure_root()?;
        assert!(root.is_dir());
        #[cfg(unix)]
        assert_eq!(fs::metadata(root)?.permissions().mode() & 0o777, 0o700);
        Ok(())
    }

    #[test]
    fn ensure_root_rejects_files_and_symlinks() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join("file");
        fs::write(&file, b"fixture")?;
        assert!(ImageOutputStore::new(&file, ImageOutputPolicy::standard()).is_err());
        let late_file = temp.path().join("late-file");
        let late_file_store = ImageOutputStore::new(&late_file, ImageOutputPolicy::standard())?;
        fs::write(&late_file, b"fixture")?;
        assert!(late_file_store.ensure_root().is_err());
        #[cfg(unix)]
        {
            let target = temp.path().join("target");
            fs::create_dir(&target)?;
            let link = temp.path().join("link");
            symlink(&target, &link)?;
            assert!(ImageOutputStore::new(link, ImageOutputPolicy::standard()).is_err());
            let late_link = temp.path().join("late-link");
            let late_link_store = ImageOutputStore::new(&late_link, ImageOutputPolicy::standard())?;
            symlink(&target, &late_link)?;
            assert!(late_link_store.ensure_root().is_err());
        }
        Ok(())
    }
}
