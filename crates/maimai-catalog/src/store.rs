use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use arc_swap::ArcSwap;
use thiserror::Error;
use tokio::{sync::Mutex, task};

use crate::{
    AddAliasRequest, AddAliasResult, AliasError, AliasKind, AliasListRequest, AliasListResult,
    CatalogError, CatalogFiles, CatalogSnapshot, DeleteAliasRequest, DeleteAliasResult,
    alias::AliasStore,
};

pub struct CatalogStore {
    aliases: AliasStore,
    published: ArcSwap<PublishedCatalog>,
    reload_lock: Mutex<()>,
}

struct PublishedCatalog {
    snapshot: Arc<CatalogSnapshot>,
    revision: CatalogRevision,
}

impl CatalogStore {
    pub async fn load(files: CatalogFiles) -> Result<Self, CatalogStoreError> {
        let (snapshot, revision) = build_snapshot(files.clone()).await?;
        Ok(Self {
            aliases: AliasStore::new(files),
            published: ArcSwap::from(Arc::new(PublishedCatalog {
                snapshot: Arc::new(snapshot),
                revision,
            })),
            reload_lock: Mutex::new(()),
        })
    }

    /// Lock-free read of the currently published immutable snapshot.
    pub fn snapshot(&self) -> Arc<CatalogSnapshot> {
        Arc::clone(&self.published.load_full().snapshot)
    }

    pub fn files(&self) -> &CatalogFiles {
        self.aliases.files()
    }

    /// Rebuild and atomically publish a complete snapshot.
    ///
    /// The mutex serializes only reload work. Readers never acquire it.
    pub async fn reload(&self) -> Result<Arc<CatalogSnapshot>, CatalogStoreError> {
        let _guard = self.reload_lock.lock().await;
        self.reload_locked().await
    }

    /// Reload only when at least one source file's size or modification time changed.
    /// Concurrent callers re-check the revision after entering the single-flight lock.
    pub async fn reload_if_changed(&self) -> Result<ReloadOutcome, CatalogStoreError> {
        let _guard = self.reload_lock.lock().await;
        let observed = read_revision_async(self.files().clone()).await?;
        let published = self.published.load_full();
        if published.revision == observed {
            return Ok(ReloadOutcome::Unchanged(Arc::clone(&published.snapshot)));
        }
        self.reload_locked().await.map(ReloadOutcome::Reloaded)
    }

    async fn reload_locked(&self) -> Result<Arc<CatalogSnapshot>, CatalogStoreError> {
        let (next, revision) = build_snapshot(self.files().clone()).await?;
        let next = Arc::new(next);
        self.published.store(Arc::new(PublishedCatalog {
            snapshot: Arc::clone(&next),
            revision,
        }));
        Ok(next)
    }

    pub async fn add_alias(
        &self,
        request: AddAliasRequest,
    ) -> Result<AddAliasResult, CatalogStoreError> {
        let _guard = self.reload_lock.lock().await;
        let aliases = self.aliases.clone();
        let snapshot = self.snapshot();
        let result = task::spawn_blocking(move || aliases.add(&snapshot, request))
            .await
            .map_err(CatalogStoreError::Join)??;
        if result.outcome().changed() || self.revision_changed_locked().await? {
            self.publish_alias_reload(result.kind(), result.document())
                .await?;
        }
        Ok(result)
    }

    pub async fn delete_alias(
        &self,
        request: DeleteAliasRequest,
    ) -> Result<DeleteAliasResult, CatalogStoreError> {
        let _guard = self.reload_lock.lock().await;
        let aliases = self.aliases.clone();
        let snapshot = self.snapshot();
        let result = task::spawn_blocking(move || aliases.delete(&snapshot, request))
            .await
            .map_err(CatalogStoreError::Join)??;
        self.publish_alias_reload(result.kind(), result.document())
            .await?;
        Ok(result)
    }

    pub async fn list_aliases(
        &self,
        request: AliasListRequest,
    ) -> Result<AliasListResult, CatalogStoreError> {
        let _guard = self.reload_lock.lock().await;
        let aliases = self.aliases.clone();
        let snapshot = self.snapshot();
        task::spawn_blocking(move || aliases.list(&snapshot, request))
            .await
            .map_err(CatalogStoreError::Join)?
            .map_err(CatalogStoreError::Alias)
    }

    async fn publish_alias_reload(
        &self,
        kind: AliasKind,
        document: &Path,
    ) -> Result<(), CatalogStoreError> {
        self.reload_locked().await.map(|_| ()).map_err(|source| {
            CatalogStoreError::AliasWrittenReloadFailed {
                kind,
                path: document.to_owned(),
                source: Box::new(source),
            }
        })
    }

    async fn revision_changed_locked(&self) -> Result<bool, CatalogStoreError> {
        let observed = read_revision_async(self.files().clone()).await?;
        Ok(self.published.load_full().revision != observed)
    }
}

#[derive(Clone, Debug)]
pub enum ReloadOutcome {
    Unchanged(Arc<CatalogSnapshot>),
    Reloaded(Arc<CatalogSnapshot>),
}

impl ReloadOutcome {
    pub const fn changed(&self) -> bool {
        matches!(self, Self::Reloaded(_))
    }

    pub fn snapshot(&self) -> &Arc<CatalogSnapshot> {
        match self {
            Self::Unchanged(snapshot) | Self::Reloaded(snapshot) => snapshot,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CatalogRevision {
    lxns_song_list: FileRevision,
    diving_fish_song_list: FileRevision,
    lxns_alias_list: FileRevision,
    yuzu_alias_list: FileRevision,
    custom_aliases: FileRevision,
    pinyin_aliases: FileRevision,
    simplified_to_traditional: FileRevision,
    dxdata: Option<FileRevision>,
    chart_stats: Option<FileRevision>,
    tags: Option<FileRevision>,
    official_music_data: Option<FileRevision>,
    dxrating_aliases: Option<FileRevision>,
    legacy_aliases_csv: Option<FileRevision>,
    artist_aliases: Option<FileRevision>,
    charter_aliases: Option<FileRevision>,
    traditional_to_simplified: Option<FileRevision>,
    maimaidxplate: Option<FileRevision>,
    custom_plates: Option<FileRevision>,
}

impl CatalogRevision {
    fn read(files: &CatalogFiles) -> Result<Self, CatalogStoreError> {
        Ok(Self {
            lxns_song_list: FileRevision::read(&files.lxns_song_list)?,
            diving_fish_song_list: FileRevision::read(&files.diving_fish_song_list)?,
            lxns_alias_list: FileRevision::read(&files.lxns_alias_list)?,
            yuzu_alias_list: FileRevision::read(&files.yuzu_alias_list)?,
            custom_aliases: FileRevision::read(&files.custom_aliases)?,
            pinyin_aliases: FileRevision::read(&files.pinyin_aliases)?,
            simplified_to_traditional: FileRevision::read(&files.simplified_to_traditional)?,
            dxdata: FileRevision::read_optional(files.dxdata.as_deref())?,
            chart_stats: FileRevision::read_optional(files.chart_stats.as_deref())?,
            tags: FileRevision::read_optional(files.tags.as_deref())?,
            official_music_data: FileRevision::read_optional(files.official_music_data.as_deref())?,
            dxrating_aliases: FileRevision::read_optional(files.dxrating_aliases.as_deref())?,
            legacy_aliases_csv: FileRevision::read_optional(files.legacy_aliases_csv.as_deref())?,
            artist_aliases: FileRevision::read_optional(files.artist_aliases.as_deref())?,
            charter_aliases: FileRevision::read_optional(files.charter_aliases.as_deref())?,
            traditional_to_simplified: FileRevision::read_optional(
                files.traditional_to_simplified.as_deref(),
            )?,
            maimaidxplate: FileRevision::read_optional(files.maimaidxplate.as_deref())?,
            custom_plates: FileRevision::read_optional(files.custom_plates.as_deref())?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileRevision {
    size: u64,
    modified: SystemTime,
}

impl FileRevision {
    fn read(path: &Path) -> Result<Self, CatalogStoreError> {
        let metadata = fs::metadata(path).map_err(|source| CatalogStoreError::Metadata {
            path: path.to_owned(),
            source,
        })?;
        let modified = metadata
            .modified()
            .map_err(|source| CatalogStoreError::Metadata {
                path: path.to_owned(),
                source,
            })?;
        Ok(Self {
            size: metadata.len(),
            modified,
        })
    }

    fn read_optional(path: Option<&Path>) -> Result<Option<Self>, CatalogStoreError> {
        let Some(path) = path else {
            return Ok(None);
        };
        match Self::read(path) {
            Ok(value) => Ok(Some(value)),
            Err(CatalogStoreError::Metadata { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Error)]
pub enum CatalogStoreError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),

    #[error(transparent)]
    Alias(#[from] AliasError),

    #[error("读取曲库文件元数据失败：{path}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("曲库文件在加载期间发生变化；保留当前快照")]
    FilesChangedDuringLoad,

    #[error("曲库后台加载任务失败")]
    Join(#[source] task::JoinError),

    #[error(
        "{kind} alias file was updated at {path}, but rebuilding the catalog failed; the previously published snapshot is still active"
    )]
    AliasWrittenReloadFailed {
        kind: AliasKind,
        path: PathBuf,
        #[source]
        source: Box<CatalogStoreError>,
    },
}

async fn build_snapshot(
    files: CatalogFiles,
) -> Result<(CatalogSnapshot, CatalogRevision), CatalogStoreError> {
    task::spawn_blocking(move || {
        let before = CatalogRevision::read(&files)?;
        let snapshot = files.load()?;
        let after = CatalogRevision::read(&files)?;
        if before != after {
            return Err(CatalogStoreError::FilesChangedDuringLoad);
        }
        Ok((snapshot, after))
    })
    .await
    .map_err(CatalogStoreError::Join)?
}

async fn read_revision_async(files: CatalogFiles) -> Result<CatalogRevision, CatalogStoreError> {
    task::spawn_blocking(move || CatalogRevision::read(&files))
        .await
        .map_err(CatalogStoreError::Join)?
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
