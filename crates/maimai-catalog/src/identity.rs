use std::collections::HashMap;

use maimai_core::{Music, SongIdNamespace, SongIdValue};

use crate::{CatalogError, TextNormalizer, raw::DxSong};

/// Snapshot-local identity index. Numeric source IDs are authoritative; a title
/// is used only when it identifies exactly one normalized song.
pub(crate) struct SongIdentityIndex {
    lxns_numeric: HashMap<u32, Vec<usize>>,
    diving_fish_numeric: HashMap<u32, Vec<usize>>,
    titles: HashMap<String, Vec<usize>>,
}

impl SongIdentityIndex {
    pub(crate) fn build(songs: &[Music], normalizer: &TextNormalizer) -> Self {
        let mut result = Self {
            lxns_numeric: HashMap::new(),
            diving_fish_numeric: HashMap::new(),
            titles: HashMap::new(),
        };
        for (index, song) in songs.iter().enumerate() {
            result.insert(index, song, normalizer);
        }
        result
    }

    pub(crate) fn insert(&mut self, index: usize, song: &Music, normalizer: &TextNormalizer) {
        push_index(
            self.titles
                .entry(normalizer.normalize(&song.title))
                .or_default(),
            index,
        );
        for source_id in &song.source_ids {
            if let SongIdValue::Numeric(value) = source_id.value() {
                let target = match source_id.namespace() {
                    SongIdNamespace::Lxns => Some(&mut self.lxns_numeric),
                    SongIdNamespace::DivingFish => Some(&mut self.diving_fish_numeric),
                    _ => None,
                };
                if let Some(target) = target {
                    push_index(target.entry(*value).or_default(), index);
                }
            }
        }
    }

    pub(crate) fn by_lxns(&self, value: u32) -> Option<usize> {
        unique(self.lxns_numeric.get(&value))
    }

    pub(crate) fn by_diving_fish(&self, value: u32, is_dx: bool) -> Option<usize> {
        let target = self
            .diving_fish_numeric
            .get(&value)
            .and_then(|values| unique(Some(values)));
        target.or_else(|| {
            let base = if is_dx && (10_000..100_000).contains(&value) {
                value - 10_000
            } else {
                value
            };
            self.by_lxns(base)
        })
    }

    pub(crate) fn by_official(&self, value: u32) -> Option<usize> {
        self.by_lxns(value)
            .or_else(|| unique(self.diving_fish_numeric.get(&value)))
    }

    pub(crate) fn resolve_dx(
        &self,
        raw: &DxSong,
        normalizer: &TextNormalizer,
    ) -> Result<Option<usize>, CatalogError> {
        let mut candidates = Vec::new();
        for sheet in &raw.sheets {
            let Some(internal_id) = sheet.internal_id else {
                continue;
            };
            let base = match sheet.chart_type.trim().to_ascii_lowercase().as_str() {
                "dx" if (10_000..100_000).contains(&internal_id) => internal_id - 10_000,
                "std" | "utage" | "utage1p" | "utage2p" | "dx" => internal_id,
                _ => continue,
            };
            if let Some(value) = self.by_lxns(base) {
                push_index(&mut candidates, value);
            }
            if let Some(value) = unique(self.diving_fish_numeric.get(&internal_id)) {
                push_index(&mut candidates, value);
            }
        }
        match candidates.as_slice() {
            [index] => return Ok(Some(*index)),
            [] => {}
            _ => return Err(ambiguous("dxdata", raw, candidates)),
        }

        let title = normalizer.normalize(&raw.title);
        let candidates = self.titles.get(&title).cloned().unwrap_or_default();
        match candidates.as_slice() {
            [index] => Ok(Some(*index)),
            [] => Ok(None),
            _ => Err(ambiguous("dxdata", raw, candidates)),
        }
    }

    pub(crate) fn resolve_title(
        &self,
        source_name: &'static str,
        source_id: &str,
        title: &str,
        normalizer: &TextNormalizer,
    ) -> Result<Option<usize>, CatalogError> {
        let candidates = self
            .titles
            .get(&normalizer.normalize(title))
            .cloned()
            .unwrap_or_default();
        match candidates.as_slice() {
            [index] => Ok(Some(*index)),
            [] => Ok(None),
            _ => Err(CatalogError::AmbiguousSongIdentity {
                source_name,
                song_id: source_id.to_owned(),
                title: title.to_owned(),
                candidates,
            }),
        }
    }
}

fn ambiguous(source_name: &'static str, raw: &DxSong, candidates: Vec<usize>) -> CatalogError {
    CatalogError::AmbiguousSongIdentity {
        source_name,
        song_id: raw.song_id.clone(),
        title: raw.title.clone(),
        candidates,
    }
}

fn unique(values: Option<&Vec<usize>>) -> Option<usize> {
    values.and_then(|values| match values.as_slice() {
        [value] => Some(*value),
        _ => None,
    })
}

fn push_index(values: &mut Vec<usize>, value: usize) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use maimai_core::{Music, SongIdNamespace, SourceSongId};

    use super::SongIdentityIndex;
    use crate::{
        TextNormalizer,
        raw::{DxNoteCounts, DxRegions, DxSheet, DxSong},
    };

    #[test]
    fn dx_internal_id_resolves_duplicate_titles_without_last_wins()
    -> Result<(), crate::CatalogError> {
        let songs = vec![song(131, "Link"), song(383, "Link")];
        let index = SongIdentityIndex::build(&songs, &TextNormalizer::default());
        let raw = dx_song("Link", 131);
        assert_eq!(index.resolve_dx(&raw, &TextNormalizer::default())?, Some(0));
        let raw = dx_song("Link (2)", 383);
        assert_eq!(index.resolve_dx(&raw, &TextNormalizer::default())?, Some(1));
        Ok(())
    }

    #[test]
    fn numeric_offset_is_adapter_scoped_not_namespace_global() {
        let songs = vec![song(1, "Base"), song(10_001, "Actual 10001")];
        let index = SongIdentityIndex::build(&songs, &TextNormalizer::default());
        assert_eq!(index.by_lxns(10_001), Some(1));
        assert_eq!(index.by_official(10_001), Some(1));
        assert_eq!(index.by_diving_fish(10_001, true), Some(0));
    }

    fn song(id: u32, title: &str) -> Music {
        let source_id = SourceSongId::numeric(SongIdNamespace::Lxns, id);
        Music {
            primary_id: source_id.clone(),
            source_ids: vec![source_id],
            title: title.to_owned(),
            artist: String::new(),
            genre: String::new(),
            version: String::new(),
            bpm: 0,
            aliases: Vec::new(),
            charts: Vec::new(),
        }
    }

    fn dx_song(song_id: &str, internal_id: u32) -> DxSong {
        DxSong {
            song_id: song_id.to_owned(),
            title: "Link".to_owned(),
            artist: String::new(),
            category: String::new(),
            image_name: None,
            bpm: None,
            search_acronyms: Vec::new(),
            is_new: false,
            is_locked: false,
            sheets: vec![DxSheet {
                chart_type: "std".to_owned(),
                difficulty: "master".to_owned(),
                level: "12".to_owned(),
                internal_level_value: None,
                note_designer: None,
                note_counts: DxNoteCounts::default(),
                regions: DxRegions::default(),
                region_overrides: BTreeMap::new(),
                is_special: false,
                version: String::new(),
                internal_id: Some(internal_id),
                release_date: None,
                multiver_internal_level_value: BTreeMap::new(),
            }],
        }
    }
}
