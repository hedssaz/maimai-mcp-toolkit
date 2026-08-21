use std::path::{Path, PathBuf};

use time::{Duration, OffsetDateTime};

use super::B50ImageError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputPolicy(crate::image_output::ImageOutputPolicy);

impl OutputPolicy {
    pub fn new(ttl: Duration, max_files: usize) -> Result<Self, B50ImageError> {
        crate::image_output::ImageOutputPolicy::new(ttl, max_files)
            .map(Self)
            .map_err(Into::into)
    }

    pub const fn standard() -> Self {
        Self(crate::image_output::ImageOutputPolicy::standard())
    }
}

#[derive(Clone, Debug)]
pub struct OutputStore(crate::image_output::ImageOutputStore);

pub use crate::image_output::SavedImage;

impl OutputStore {
    pub fn from_shared(store: crate::image_output::ImageOutputStore) -> Self {
        Self(store)
    }

    pub fn new(root: impl Into<PathBuf>, policy: OutputPolicy) -> Result<Self, B50ImageError> {
        crate::image_output::ImageOutputStore::new(root, policy.0)
            .map(Self)
            .map_err(Into::into)
    }

    pub fn root(&self) -> &Path {
        self.0.root()
    }

    pub fn save_png(
        &self,
        filename_stem: &str,
        png: &[u8],
        now: OffsetDateTime,
    ) -> Result<SavedImage, B50ImageError> {
        self.0.save_png(filename_stem, png, now).map_err(Into::into)
    }

    pub fn cleanup(&self, now: OffsetDateTime) -> Result<usize, B50ImageError> {
        self.0.cleanup(now).map_err(Into::into)
    }
}

impl From<crate::image_output::ImageOutputStore> for OutputStore {
    fn from(value: crate::image_output::ImageOutputStore) -> Self {
        Self::from_shared(value)
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fs};

    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn shared_output_store_preserves_root_save_and_cleanup() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let shared = crate::image_output::ImageOutputStore::new(
            temp.path(),
            crate::image_output::ImageOutputPolicy::standard(),
        )?;
        let output = OutputStore::from_shared(shared.clone());
        assert_eq!(output.root(), shared.root());

        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000)?;
        let saved = output.save_png("shared", b"\x89PNG\r\n\x1a\nfixture", now)?;
        assert!(saved.path.exists());
        assert_eq!(shared.cleanup(now)?, 0);
        fs::remove_file(saved.path)?;
        Ok(())
    }
}
