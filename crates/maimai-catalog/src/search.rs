use std::collections::HashMap;

use maimai_core::{Music, SongIdValue};

use crate::{
    CatalogSnapshot, TextNormalizer,
    query::{CatalogQuery, SearchHit, execute_supported},
};

#[derive(Clone, Debug)]
pub(crate) struct SearchIndex {
    entries: Vec<SearchEntry>,
    numeric_ids: HashMap<u32, Vec<usize>>,
}

#[derive(Clone, Debug)]
pub(crate) struct SearchEntry {
    pub(crate) title: String,
    pub(crate) title_display: String,
    pub(crate) aliases: Vec<SearchTerm>,
    pub(crate) pinyin: Vec<SearchTerm>,
    pub(crate) keywords: Vec<SearchTerm>,
}

#[derive(Clone, Debug)]
pub(crate) struct SearchTerm {
    pub(crate) normalized: String,
    pub(crate) display: String,
}

impl SearchIndex {
    pub(crate) fn build(
        songs: &[Music],
        normalizer: &TextNormalizer,
        hidden_pinyin: &[Vec<String>],
        hidden_keywords: &[Vec<String>],
        searchable_numeric_ids: &[std::collections::BTreeSet<u32>],
    ) -> Self {
        let mut numeric_ids = HashMap::<u32, Vec<usize>>::new();
        let entries = songs
            .iter()
            .enumerate()
            .map(|(index, music)| {
                for source_id in &music.source_ids {
                    if let SongIdValue::Numeric(value) = source_id.value() {
                        numeric_ids.entry(*value).or_default().push(index);
                    }
                }
                if let Some(values) = searchable_numeric_ids.get(index) {
                    for value in values {
                        let indices = numeric_ids.entry(*value).or_default();
                        if !indices.contains(&index) {
                            indices.push(index);
                        }
                    }
                }
                SearchEntry {
                    title: normalizer.normalize(&music.title),
                    title_display: music.title.clone(),
                    aliases: music
                        .aliases
                        .iter()
                        .map(|alias| SearchTerm {
                            normalized: normalizer.normalize(alias),
                            display: alias.clone(),
                        })
                        .collect(),
                    pinyin: terms(hidden_pinyin.get(index)),
                    keywords: terms(hidden_keywords.get(index)),
                }
            })
            .collect();
        Self {
            entries,
            numeric_ids,
        }
    }

    pub(crate) fn entry(&self, index: usize) -> Option<&SearchEntry> {
        self.entries.get(index)
    }

    pub(crate) fn has_numeric_id(&self, index: usize, id: u32) -> bool {
        self.numeric_ids
            .get(&id)
            .is_some_and(|indices| indices.contains(&index))
    }
}

fn terms(values: Option<&Vec<String>>) -> Vec<SearchTerm> {
    values
        .into_iter()
        .flatten()
        .map(|value| SearchTerm {
            normalized: value.clone(),
            display: value.clone(),
        })
        .collect()
}

impl CatalogSnapshot {
    /// Compatibility wrapper around the shared typed query engine.
    pub fn search_text(&self, query: &str, limit: usize) -> Vec<SearchHit<'_>> {
        if limit == 0 {
            return Vec::new();
        }
        execute_supported(self, &CatalogQuery::text(query, limit))
    }
}
