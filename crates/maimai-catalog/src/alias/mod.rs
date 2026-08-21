mod error;
mod file;
mod name;
mod song;
mod types;

pub use error::AliasError;
pub use types::{
    AddAliasOutcome, AddAliasRequest, AddAliasResult, AliasKind, AliasListRequest, AliasListResult,
    AliasSong, AliasText, CanonicalName, DeleteAliasRequest, DeleteAliasResult, NameAliasEntry,
    NameAliasMutation, SongAliasEntry, SongAliasMutation, SongAliasTarget, SongTitle,
};

use crate::{CatalogFiles, CatalogSnapshot};

#[derive(Clone)]
pub(crate) struct AliasStore {
    files: CatalogFiles,
}

impl AliasStore {
    pub(crate) const fn new(files: CatalogFiles) -> Self {
        Self { files }
    }

    pub(crate) const fn files(&self) -> &CatalogFiles {
        &self.files
    }

    pub(crate) fn add(
        &self,
        snapshot: &CatalogSnapshot,
        request: AddAliasRequest,
    ) -> Result<AddAliasResult, AliasError> {
        match request.kind() {
            AliasKind::Song => song::add(&self.files, snapshot, request),
            AliasKind::Artist | AliasKind::Charter => name::add(&self.files, snapshot, request),
        }
    }

    pub(crate) fn delete(
        &self,
        snapshot: &CatalogSnapshot,
        request: DeleteAliasRequest,
    ) -> Result<DeleteAliasResult, AliasError> {
        match request.kind() {
            AliasKind::Song => song::delete(&self.files, snapshot, request),
            AliasKind::Artist | AliasKind::Charter => name::delete(&self.files, snapshot, request),
        }
    }

    pub(crate) fn list(
        &self,
        snapshot: &CatalogSnapshot,
        request: AliasListRequest,
    ) -> Result<AliasListResult, AliasError> {
        match request.kind() {
            AliasKind::Song => song::list(snapshot, request),
            AliasKind::Artist | AliasKind::Charter => name::list(&self.files, snapshot, request),
        }
    }
}

#[cfg(test)]
mod tests;
