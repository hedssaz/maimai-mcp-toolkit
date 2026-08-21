use std::path::PathBuf;

use thiserror::Error;

use super::AliasKind;

#[derive(Debug, Error)]
pub enum AliasError {
    #[error("kind must be song, artist, or charter: {0}")]
    InvalidKind(String),
    #[error("{field} is required")]
    Empty { field: &'static str },
    #[error("{field} contains a control character")]
    ControlCharacter { field: &'static str },
    #[error("song_id or title is required")]
    SongTargetRequired,
    #[error("query, song_id, or title is required")]
    SongListQueryRequired,
    #[error("kind must be artist or charter")]
    NameKindRequired,
    #[error("{kind} canonical name is required")]
    CanonicalRequired { kind: AliasKind },
    #[error("count/limit must be between 1 and 200")]
    InvalidLimit(usize),
    #[error("song not found")]
    SongNotFound,
    #[error("multiple songs matched {target:?}; use song_id instead")]
    AmbiguousSong { target: String },
    #[error("no {kind} aliases file found")]
    AliasFileMissing { kind: AliasKind },
    #[error("no alias file configured for {kind}")]
    AliasFileNotConfigured { kind: AliasKind },
    #[error("invalid {kind} aliases file: {path}")]
    InvalidDocument { kind: AliasKind, path: PathBuf },
    #[error("no custom aliases for song {song_id}")]
    NoSongAliases { song_id: String },
    #[error("no aliases for {kind} {canonical:?}")]
    NoNameAliases { kind: AliasKind, canonical: String },
    #[error("alias {alias:?} not found for song {song_id}")]
    SongAliasNotFound { alias: String, song_id: String },
    #[error("alias {alias:?} not found for {kind} {canonical:?}")]
    NameAliasNotFound {
        alias: String,
        kind: AliasKind,
        canonical: String,
    },
    #[error("failed to {operation} alias file {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to atomically replace alias file {path}")]
    Persist {
        path: PathBuf,
        #[source]
        source: tempfile::PersistError,
    },
    #[error("refusing alias path through symbolic link: {path}")]
    SymbolicLink { path: PathBuf },
    #[error("failed to parse alias file {path}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Query(#[from] crate::QueryError),
}
