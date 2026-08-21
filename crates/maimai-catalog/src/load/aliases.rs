pub(super) struct AliasIndexes {
    numeric: HashMap<u32, Vec<usize>>,
    text_ids: HashMap<String, Vec<usize>>,
    titles: HashMap<String, Vec<usize>>,
}

impl AliasIndexes {
    pub(super) fn new(songs: &[Music], normalizer: &TextNormalizer) -> Self {
        let mut numeric = HashMap::new();
        let mut text_ids = HashMap::new();
        let mut titles = HashMap::new();
        for (index, music) in songs.iter().enumerate() {
            push_unique(
                titles
                    .entry(normalizer.normalize(&music.title))
                    .or_default(),
                index,
            );
            for source_id in &music.source_ids {
                match source_id.value() {
                    SongIdValue::Numeric(song_id) => {
                        push_unique(numeric.entry(*song_id).or_default(), index);
                    }
                    SongIdValue::Text(song_id) => {
                        push_unique(
                            text_ids
                                .entry(normalizer.normalize(song_id.as_str()))
                                .or_default(),
                            index,
                        );
                    }
                }
            }
        }
        Self {
            numeric,
            text_ids,
            titles,
        }
    }

    fn resolve(&self, normalizer: &TextNormalizer, value: &Value) -> Vec<usize> {
        if let Some(number) = value.as_u64().and_then(|number| u32::try_from(number).ok()) {
            return self.numeric.get(&number).cloned().unwrap_or_default();
        }
        let Some(text) = value.as_str() else {
            return Vec::new();
        };
        let text = text.trim();
        if let Ok(number) = text.parse::<u32>() {
            return self.numeric.get(&number).cloned().unwrap_or_default();
        }
        let normalized = normalizer.normalize(text);
        self.text_ids
            .get(&normalized)
            .or_else(|| self.titles.get(&normalized))
            .cloned()
            .unwrap_or_default()
    }
}

pub(super) fn attach_alias_entries(
    songs: &mut [Music],
    normalizer: &TextNormalizer,
    indexes: &AliasIndexes,
    entries: Vec<crate::raw::AliasEntry>,
) {
    for entry in entries {
        for index in indexes.resolve(normalizer, &entry.song_id) {
            add_aliases(&mut songs[index], normalizer, entry.aliases.clone());
        }
    }
}

pub(super) fn attach_yuzu_aliases(
    songs: &mut [Music],
    normalizer: &TextNormalizer,
    indexes: &AliasIndexes,
    entries: Vec<YuzuAliasEntry>,
) {
    for entry in entries {
        for index in indexes.resolve(normalizer, &entry.song_id) {
            add_aliases(&mut songs[index], normalizer, entry.alias.clone());
        }
    }
}

pub(super) fn attach_custom_aliases(
    songs: &mut [Music],
    normalizer: &TextNormalizer,
    indexes: &AliasIndexes,
    aliases: BTreeMap<String, Vec<String>>,
) {
    for (song_id, aliases) in aliases {
        let value = Value::String(song_id);
        for index in indexes.resolve(normalizer, &value) {
            add_aliases(&mut songs[index], normalizer, aliases.clone());
        }
    }
}

pub(super) fn attach_dxrating_aliases(
    songs: &mut [Music],
    normalizer: &TextNormalizer,
    indexes: &AliasIndexes,
    entries: Vec<DxRatingAlias>,
) {
    for entry in entries {
        let title = Value::String(entry.song_id);
        for index in indexes.resolve(normalizer, &title) {
            add_aliases(&mut songs[index], normalizer, [entry.name.clone()]);
        }
    }
}

pub(super) fn attach_legacy_aliases(
    songs: &mut [Music],
    normalizer: &TextNormalizer,
    indexes: &AliasIndexes,
    source: &str,
) -> Result<(), CatalogError> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(source.as_bytes());
    for row in reader.records() {
        let row = row.map_err(CatalogError::Csv)?;
        let Some(title) = row.get(0) else {
            continue;
        };
        let value = Value::String(title.to_owned());
        let aliases = row
            .iter()
            .skip(1)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for index in indexes.resolve(normalizer, &value) {
            add_aliases(&mut songs[index], normalizer, aliases.clone());
        }
    }
    Ok(())
}

pub(super) fn collect_hidden_aliases(
    song_count: usize,
    normalizer: &TextNormalizer,
    indexes: &AliasIndexes,
    entries: Vec<crate::raw::AliasEntry>,
) -> Vec<Vec<String>> {
    let mut result = vec![Vec::new(); song_count];
    for entry in entries {
        for index in indexes.resolve(normalizer, &entry.song_id) {
            let Some(target) = result.get_mut(index) else {
                continue;
            };
            for alias in &entry.aliases {
                let alias = alias.trim();
                if alias.is_empty() {
                    continue;
                }
                let normalized = normalizer.normalize(alias);
                if !normalized.is_empty() && !target.contains(&normalized) {
                    target.push(normalized);
                }
            }
        }
    }
    result
}

pub(super) fn build_name_aliases(
    source: NameAliasDocument,
    normalizer: &TextNormalizer,
) -> HashMap<String, std::collections::BTreeSet<String>> {
    let mut result = HashMap::new();
    for (canonical, aliases) in source {
        let names = std::iter::once(canonical)
            .chain(aliases)
            .map(|value| normalizer.normalize(&value))
            .filter(|value| !value.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        for name in &names {
            result.insert(name.clone(), names.clone());
        }
    }
    result
}

pub(super) fn latest_cn_versions(source: &LxnsCatalog) -> Vec<u32> {
    let mut versions = source
        .songs
        .iter()
        .map(|song| song.version)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .rev();
    let Some(latest) = versions.next() else {
        return Vec::new();
    };
    let all_utage = source
        .songs
        .iter()
        .filter(|song| song.version == latest)
        .all(|song| song.difficulties.standard.is_empty() && song.difficulties.dx.is_empty());
    if all_utage {
        versions
            .next()
            .map_or_else(|| vec![latest], |previous| vec![latest, previous])
    } else {
        vec![latest]
    }
}

fn add_aliases(
    music: &mut Music,
    normalizer: &TextNormalizer,
    aliases: impl IntoIterator<Item = String>,
) {
    let mut existing = music
        .aliases
        .iter()
        .map(|alias| normalizer.normalize(alias))
        .collect::<std::collections::BTreeSet<_>>();
    for alias in aliases {
        let alias = alias.trim().to_owned();
        let normalized = normalizer.normalize(&alias);
        if !normalized.is_empty() && existing.insert(normalized) {
            music.aliases.push(alias);
        }
    }
}

fn push_unique<T: Eq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}
use std::collections::{BTreeMap, HashMap};

use maimai_core::{Music, SongIdValue};
use serde_json::Value;

use crate::{
    CatalogError, TextNormalizer,
    raw::{DxRatingAlias, LxnsCatalog, NameAliasDocument, YuzuAliasEntry},
};
