use std::path::{Path, PathBuf};

use crate::b50_image::B50ImageStyle;

use super::B50RenderError;

#[derive(Clone, Debug)]
pub struct ResourceOverridePolicy {
    legacy_static_root: PathBuf,
    static_roots: Vec<PathBuf>,
    cover_roots: Vec<PathBuf>,
}

impl ResourceOverridePolicy {
    pub fn new(
        legacy_static_root: impl Into<PathBuf>,
        static_roots: impl IntoIterator<Item = PathBuf>,
        cover_roots: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, B50RenderError> {
        let legacy_static_root = canonical_directory(&legacy_static_root.into(), "staticDir")?;
        let mut static_roots = canonical_roots(static_roots, "staticDir")?;
        if !static_roots.contains(&legacy_static_root) {
            static_roots.push(legacy_static_root.clone());
        }
        Ok(Self {
            legacy_static_root,
            static_roots,
            cover_roots: canonical_roots(cover_roots, "coverCacheDir")?,
        })
    }

    pub(crate) fn static_dir(
        &self,
        requested: Option<PathBuf>,
        style: B50ImageStyle,
    ) -> Result<Option<PathBuf>, B50RenderError> {
        let resolved = allowed(requested, &self.static_roots, "staticDir")?;
        if style == B50ImageStyle::Legacy
            && resolved
                .as_ref()
                .is_some_and(|path| path != &self.legacy_static_root)
        {
            return Err(B50RenderError::LegacyStaticOverrideUnsupported);
        }
        Ok(resolved)
    }

    pub(crate) fn cover_cache_dir(
        &self,
        requested: Option<PathBuf>,
    ) -> Result<Option<PathBuf>, B50RenderError> {
        allowed(requested, &self.cover_roots, "coverCacheDir")
    }
}

fn canonical_roots(
    roots: impl IntoIterator<Item = PathBuf>,
    field: &'static str,
) -> Result<Vec<PathBuf>, B50RenderError> {
    roots
        .into_iter()
        .map(|root| canonical_directory(&root, field))
        .collect()
}

fn allowed(
    requested: Option<PathBuf>,
    roots: &[PathBuf],
    field: &'static str,
) -> Result<Option<PathBuf>, B50RenderError> {
    let Some(requested) = requested else {
        return Ok(None);
    };
    let canonical = canonical_directory(&requested, field)?;
    if roots.iter().any(|root| canonical.starts_with(root)) {
        Ok(Some(canonical))
    } else {
        Err(B50RenderError::UnsafePath { field })
    }
}

fn canonical_directory(path: &Path, field: &'static str) -> Result<PathBuf, B50RenderError> {
    let canonical = path
        .canonicalize()
        .map_err(|_| B50RenderError::UnsafePath { field })?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(B50RenderError::UnsafePath { field })
    }
}
