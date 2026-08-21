use std::{collections::BTreeMap, fmt, path::PathBuf};

use maimai_core::{SongIdValue, SourceSongId};

use super::AliasError;
use crate::SourceKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AliasKind {
    Song,
    Artist,
    Charter,
}

impl AliasKind {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Artist => "artist",
            Self::Charter => "charter",
        }
    }

    pub fn parse(value: Option<&str>) -> Result<Self, AliasError> {
        let Some(value) = value else {
            return Ok(Self::Song);
        };
        match value.trim().to_ascii_lowercase().replace(' ', "").as_str() {
            "" | "song" => Ok(Self::Song),
            "artist" | "曲师" | "曲師" | "曲作者" | "作者" => Ok(Self::Artist),
            "charter" | "notedesigner" | "谱师" | "譜師" | "谱面作者" | "譜面作者" => {
                Ok(Self::Charter)
            }
            _ => Err(AliasError::InvalidKind(value.to_owned())),
        }
    }
}

impl fmt::Display for AliasKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AliasText(String);

impl AliasText {
    pub fn new(value: impl Into<String>) -> Result<Self, AliasError> {
        validated(value.into(), "alias").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalName(String);

impl CanonicalName {
    pub fn new(value: impl Into<String>, kind: AliasKind) -> Result<Self, AliasError> {
        let field = match kind {
            AliasKind::Artist => "artist canonical name",
            AliasKind::Charter => "charter canonical name",
            AliasKind::Song => return Err(AliasError::NameKindRequired),
        };
        validated(value.into(), field).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongTitle(String);

impl SongTitle {
    pub fn new(value: impl Into<String>) -> Result<Self, AliasError> {
        validated(value.into(), "title").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SongAliasTarget {
    Id(SongIdValue),
    Title(SongTitle),
}

#[derive(Clone, Debug)]
pub struct AddAliasRequest {
    kind: AliasKind,
    song: Option<SongAliasTarget>,
    canonical: Option<CanonicalName>,
    alias: AliasText,
}

impl AddAliasRequest {
    pub fn song(target: SongAliasTarget, alias: AliasText) -> Self {
        Self {
            kind: AliasKind::Song,
            song: Some(target),
            canonical: None,
            alias,
        }
    }

    pub fn name(
        kind: AliasKind,
        canonical: CanonicalName,
        alias: AliasText,
    ) -> Result<Self, AliasError> {
        if kind == AliasKind::Song {
            return Err(AliasError::NameKindRequired);
        }
        Ok(Self {
            kind,
            song: None,
            canonical: Some(canonical),
            alias,
        })
    }

    pub const fn kind(&self) -> AliasKind {
        self.kind
    }

    pub(crate) fn song_target(&self) -> Result<&SongAliasTarget, AliasError> {
        self.song.as_ref().ok_or(AliasError::SongTargetRequired)
    }

    pub(crate) fn canonical(&self) -> Result<&CanonicalName, AliasError> {
        self.canonical
            .as_ref()
            .ok_or(AliasError::CanonicalRequired { kind: self.kind })
    }

    pub(crate) const fn alias(&self) -> &AliasText {
        &self.alias
    }
}

#[derive(Clone, Debug)]
pub struct DeleteAliasRequest(AddAliasRequest);

impl DeleteAliasRequest {
    pub fn song(target: SongAliasTarget, alias: AliasText) -> Self {
        Self(AddAliasRequest::song(target, alias))
    }

    pub fn name(
        kind: AliasKind,
        canonical: CanonicalName,
        alias: AliasText,
    ) -> Result<Self, AliasError> {
        AddAliasRequest::name(kind, canonical, alias).map(Self)
    }

    pub const fn kind(&self) -> AliasKind {
        self.0.kind()
    }

    pub(crate) fn song_target(&self) -> Result<&SongAliasTarget, AliasError> {
        self.0.song_target()
    }

    pub(crate) fn canonical(&self) -> Result<&CanonicalName, AliasError> {
        self.0.canonical()
    }

    pub(crate) const fn alias(&self) -> &AliasText {
        self.0.alias()
    }
}

#[derive(Clone, Debug)]
pub struct AliasListRequest {
    kind: AliasKind,
    query: Option<AliasText>,
    limit: usize,
}

impl AliasListRequest {
    pub fn new(
        kind: AliasKind,
        query: Option<AliasText>,
        limit: usize,
    ) -> Result<Self, AliasError> {
        if kind == AliasKind::Song && query.is_none() {
            return Err(AliasError::SongListQueryRequired);
        }
        if kind == AliasKind::Song && !(1..=200).contains(&limit) {
            return Err(AliasError::InvalidLimit(limit));
        }
        Ok(Self { kind, query, limit })
    }

    pub const fn kind(&self) -> AliasKind {
        self.kind
    }

    pub fn query(&self) -> Option<&str> {
        self.query.as_ref().map(AliasText::as_str)
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddAliasOutcome {
    Added,
    AlreadyExists,
}

impl AddAliasOutcome {
    pub const fn changed(self) -> bool {
        matches!(self, Self::Added)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AliasSong {
    pub song_id: SongIdValue,
    pub source_id: SourceSongId,
    pub source_ids: BTreeMap<SourceKind, SourceSongId>,
    pub title: String,
    pub artist: String,
    pub source_labels: Vec<SourceKind>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongAliasMutation {
    pub song: AliasSong,
    pub alias: String,
    pub aliases: Vec<String>,
    pub document: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameAliasMutation {
    pub kind: AliasKind,
    pub canonical: String,
    pub alias: String,
    pub aliases: Vec<String>,
    pub document: PathBuf,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AddAliasResult {
    Song {
        outcome: AddAliasOutcome,
        value: SongAliasMutation,
    },
    Name {
        outcome: AddAliasOutcome,
        value: NameAliasMutation,
    },
}

impl AddAliasResult {
    pub const fn outcome(&self) -> AddAliasOutcome {
        match self {
            Self::Song { outcome, .. } | Self::Name { outcome, .. } => *outcome,
        }
    }

    pub fn document(&self) -> &PathBuf {
        match self {
            Self::Song { value, .. } => &value.document,
            Self::Name { value, .. } => &value.document,
        }
    }

    pub const fn kind(&self) -> AliasKind {
        match self {
            Self::Song { .. } => AliasKind::Song,
            Self::Name { value, .. } => value.kind,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeleteAliasResult {
    Song(SongAliasMutation),
    Name(NameAliasMutation),
}

impl DeleteAliasResult {
    pub fn document(&self) -> &PathBuf {
        match self {
            Self::Song(value) => &value.document,
            Self::Name(value) => &value.document,
        }
    }

    pub const fn kind(&self) -> AliasKind {
        match self {
            Self::Song(_) => AliasKind::Song,
            Self::Name(value) => value.kind,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongAliasEntry {
    pub song: AliasSong,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameAliasEntry {
    pub canonical: String,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AliasListResult {
    Songs {
        query: String,
        limit: usize,
        total_matches: usize,
        truncated: bool,
        entries: Vec<SongAliasEntry>,
    },
    Names {
        kind: AliasKind,
        entries: Vec<NameAliasEntry>,
        document: PathBuf,
    },
}

fn validated(value: String, field: &'static str) -> Result<String, AliasError> {
    if value.chars().any(char::is_control) {
        return Err(AliasError::ControlCharacter { field });
    }
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AliasError::Empty { field });
    }
    Ok(value)
}
