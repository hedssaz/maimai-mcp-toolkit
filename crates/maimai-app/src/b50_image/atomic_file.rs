use std::path::{Path, PathBuf};

use super::B50ImageError;

pub(super) fn secure_absolute(path: PathBuf) -> Result<PathBuf, B50ImageError> {
    crate::image_output::secure_absolute(path).map_err(Into::into)
}

pub(super) fn reject_non_regular(path: &Path) -> Result<bool, B50ImageError> {
    crate::image_output::reject_non_regular(path).map_err(Into::into)
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), B50ImageError> {
    crate::image_output::atomic_write(path, bytes).map_err(Into::into)
}
