use std::{collections::HashMap, fs, path::Path};

use serde::de::DeserializeOwned;

use crate::{CatalogError, raw::CharacterMap};

pub(super) struct CatalogDocuments<'a> {
    pub(super) lxns_songs: &'a str,
    pub(super) diving_fish_songs: &'a str,
    pub(super) lxns_aliases: &'a str,
    pub(super) yuzu_aliases: &'a str,
    pub(super) custom_aliases: &'a str,
    pub(super) pinyin_aliases: &'a str,
    pub(super) simplified_to_traditional: &'a str,
    pub(super) dxdata: Option<&'a str>,
    pub(super) chart_stats: Option<&'a str>,
    pub(super) tags: Option<&'a str>,
    pub(super) official_music_data: Option<&'a str>,
    pub(super) dxrating_aliases: Option<&'a str>,
    pub(super) legacy_aliases_csv: Option<&'a str>,
    pub(super) artist_aliases: Option<&'a str>,
    pub(super) charter_aliases: Option<&'a str>,
    pub(super) traditional_to_simplified: Option<&'a str>,
    pub(super) maimaidxplate: Option<&'a str>,
    pub(super) custom_plates: Option<&'a str>,
}

pub(super) fn read_document(path: &Path) -> Result<String, CatalogError> {
    fs::read_to_string(path).map_err(|source| CatalogError::Read {
        path: path.to_owned(),
        source,
    })
}

pub(super) fn read_optional_document(path: Option<&Path>) -> Result<Option<String>, CatalogError> {
    let Some(path) = path else {
        return Ok(None);
    };
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CatalogError::Read {
            path: path.to_owned(),
            source,
        }),
    }
}

pub(super) fn parse_json<T: DeserializeOwned>(
    document: &'static str,
    source: &str,
) -> Result<T, CatalogError> {
    serde_json::from_str(source).map_err(|source| CatalogError::Json { document, source })
}

pub(super) fn parse_optional_json<T: DeserializeOwned + Default>(
    document: &'static str,
    source: Option<&str>,
) -> Result<T, CatalogError> {
    source.map_or_else(|| Ok(T::default()), |source| parse_json(document, source))
}

pub(super) fn build_character_map(
    source: CharacterMap,
) -> Result<HashMap<char, char>, CatalogError> {
    let mut result = HashMap::with_capacity(source.len());
    for (from, to) in source {
        let mut from_chars = from.chars();
        let mut to_chars = to.chars();
        let from_char = from_chars.next();
        let to_char = to_chars.next();
        if from_chars.next().is_some()
            || to_chars.next().is_some()
            || from_char.is_none()
            || to_char.is_none()
        {
            return Err(CatalogError::InvalidCharacterMapping { from, to });
        }
        if let (Some(from_char), Some(to_char)) = (from_char, to_char) {
            result.insert(from_char, to_char);
        }
    }
    Ok(result)
}
