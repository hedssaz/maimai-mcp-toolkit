use std::path::Path;

use crate::{CatalogFiles, CatalogSnapshot};

use super::{
    AddAliasOutcome, AddAliasRequest, AddAliasResult, AliasError, AliasKind, AliasListRequest,
    AliasListResult, DeleteAliasRequest, DeleteAliasResult, NameAliasEntry, NameAliasMutation,
    file,
};

pub(super) fn add(
    files: &CatalogFiles,
    snapshot: &CatalogSnapshot,
    request: AddAliasRequest,
) -> Result<AddAliasResult, AliasError> {
    let kind = request.kind();
    let configured = configured_path(files, kind)?;
    let (path, mut document) = file::read(configured, kind, true)?;
    let canonical = request.canonical()?.as_str();
    let alias = request.alias().as_str();
    let normalized_alias = snapshot.normalizer().normalize(alias);
    let aliases = document.entry(canonical.to_owned()).or_default();
    let existing = snapshot.normalizer().normalize(canonical) == normalized_alias
        || aliases
            .iter()
            .any(|value| snapshot.normalizer().normalize(value) == normalized_alias);
    let outcome = if existing {
        AddAliasOutcome::AlreadyExists
    } else {
        aliases.push(alias.to_owned());
        file::write_atomic(&path, &document)?;
        AddAliasOutcome::Added
    };
    let aliases = document
        .get(canonical)
        .into_iter()
        .flatten()
        .map(|value| snapshot.to_simplified(value))
        .collect();
    Ok(AddAliasResult::Name {
        outcome,
        value: NameAliasMutation {
            kind,
            canonical: canonical.to_owned(),
            alias: alias.to_owned(),
            aliases,
            document: configured.to_owned(),
            warning: canonical_warning(snapshot, kind, canonical),
        },
    })
}

pub(super) fn delete(
    files: &CatalogFiles,
    snapshot: &CatalogSnapshot,
    request: DeleteAliasRequest,
) -> Result<DeleteAliasResult, AliasError> {
    let kind = request.kind();
    let configured = configured_path(files, kind)?;
    let (path, mut document) = file::read(configured, kind, false)?;
    let canonical = request.canonical()?.as_str();
    let aliases = document
        .get_mut(canonical)
        .filter(|aliases| !aliases.is_empty())
        .ok_or_else(|| AliasError::NoNameAliases {
            kind,
            canonical: canonical.to_owned(),
        })?;
    let normalized = snapshot.normalizer().normalize(request.alias().as_str());
    let position = aliases
        .iter()
        .position(|value| snapshot.normalizer().normalize(value) == normalized)
        .ok_or_else(|| AliasError::NameAliasNotFound {
            alias: request.alias().as_str().to_owned(),
            kind,
            canonical: canonical.to_owned(),
        })?;
    let removed = aliases.remove(position);
    let remaining = aliases
        .iter()
        .map(|value| snapshot.to_simplified(value))
        .collect::<Vec<_>>();
    if aliases.is_empty() {
        document.remove(canonical);
    }
    file::write_atomic(&path, &document)?;
    Ok(DeleteAliasResult::Name(NameAliasMutation {
        kind,
        canonical: canonical.to_owned(),
        alias: removed,
        aliases: remaining,
        document: configured.to_owned(),
        warning: None,
    }))
}

pub(super) fn list(
    files: &CatalogFiles,
    snapshot: &CatalogSnapshot,
    request: AliasListRequest,
) -> Result<AliasListResult, AliasError> {
    let kind = request.kind();
    let configured = configured_path(files, kind)?;
    let (_, document) = file::read(configured, kind, true)?;
    let needle = request
        .query()
        .map(|value| snapshot.normalizer().normalize(value));
    let entries = document
        .into_iter()
        .filter(|(canonical, aliases)| {
            needle
                .as_ref()
                .is_none_or(|needle| names_match(snapshot, needle, canonical, aliases))
        })
        .map(|(canonical, aliases)| NameAliasEntry {
            canonical,
            aliases: aliases
                .iter()
                .map(|value| snapshot.to_simplified(value))
                .collect(),
        })
        .collect();
    Ok(AliasListResult::Names {
        kind,
        entries,
        document: configured.to_owned(),
    })
}

fn configured_path(files: &CatalogFiles, kind: AliasKind) -> Result<&Path, AliasError> {
    let path = match kind {
        AliasKind::Artist => files.artist_aliases.as_deref(),
        AliasKind::Charter => files.charter_aliases.as_deref(),
        AliasKind::Song => return Err(AliasError::NameKindRequired),
    };
    path.ok_or(AliasError::AliasFileNotConfigured { kind })
}

fn names_match(
    snapshot: &CatalogSnapshot,
    needle: &str,
    canonical: &str,
    aliases: &[String],
) -> bool {
    std::iter::once(canonical)
        .chain(aliases.iter().map(String::as_str))
        .map(|value| snapshot.normalizer().normalize(value))
        .any(|value| needle.contains(&value) || value.contains(needle))
}

fn canonical_warning(
    snapshot: &CatalogSnapshot,
    kind: AliasKind,
    canonical: &str,
) -> Option<String> {
    if canonical_known(snapshot, kind, canonical) {
        return None;
    }
    Some(format!(
        "warning: canonical {canonical:?} did not match any {} in the local music library — this alias will never be hit unless the canonical name is exactly typed as it appears in the data.",
        kind.key()
    ))
}

fn canonical_known(snapshot: &CatalogSnapshot, kind: AliasKind, canonical: &str) -> bool {
    let needle = snapshot.normalizer().normalize(canonical);
    snapshot
        .songs()
        .iter()
        .enumerate()
        .any(|(index, song)| match kind {
            AliasKind::Artist => std::iter::once(song.artist.as_str())
                .chain(
                    snapshot
                        .song_metadata(index)
                        .into_iter()
                        .flat_map(|metadata| metadata.source_projections.iter())
                        .map(|source| source.artist.as_str()),
                )
                .any(|value| fuzzy(snapshot, &needle, value)),
            AliasKind::Charter => snapshot
                .song_metadata(index)
                .into_iter()
                .flat_map(|metadata| metadata.source_projections.iter())
                .flat_map(|source| source.charts.iter())
                .any(|chart| fuzzy(snapshot, &needle, &chart.note_designer)),
            AliasKind::Song => false,
        })
}

fn fuzzy(snapshot: &CatalogSnapshot, needle: &str, value: &str) -> bool {
    let value = snapshot.normalizer().normalize(value);
    !value.is_empty() && (needle.contains(&value) || value.contains(needle))
}
