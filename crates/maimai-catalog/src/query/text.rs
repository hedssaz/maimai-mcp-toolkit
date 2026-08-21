use crate::{
    CatalogSnapshot,
    search::{SearchEntry, SearchTerm},
};
use pinyin::ToPinyin;

use super::MatchKind;

pub(super) fn match_kind(
    snapshot: &CatalogSnapshot,
    song_index: usize,
    query: Option<&str>,
) -> Option<(MatchKind, Option<String>)> {
    let Some(query) = query else {
        return Some((MatchKind::FilterOnly, None));
    };
    let normalized = snapshot.normalizer().normalize(query);
    if normalized.is_empty() {
        return Some((MatchKind::TitleContains, Some(String::new())));
    }
    let entry = snapshot.search_index().entry(song_index)?;
    let numeric = normalized
        .strip_prefix("id")
        .unwrap_or(&normalized)
        .trim()
        .parse::<u32>()
        .ok();
    rank_entry(snapshot, entry, song_index, &normalized, numeric)
}

fn rank_entry(
    snapshot: &CatalogSnapshot,
    entry: &SearchEntry,
    song_index: usize,
    query: &str,
    numeric: Option<u32>,
) -> Option<(MatchKind, Option<String>)> {
    if numeric.is_some_and(|number| snapshot.search_index().has_numeric_id(song_index, number)) {
        return Some((MatchKind::NumericId, Some(query.to_owned())));
    }
    if entry.title == query {
        return Some((MatchKind::ExactTitle, Some(entry.title_display.clone())));
    }
    if entry.aliases.iter().any(|alias| alias.normalized == query) {
        return Some((
            MatchKind::ExactAlias,
            matching(&entry.aliases, query, Mode::Exact),
        ));
    }
    if entry.title.starts_with(query) {
        return Some((MatchKind::TitlePrefix, Some(entry.title_display.clone())));
    }
    if entry.title.contains(query) {
        return Some((MatchKind::TitleContains, Some(entry.title_display.clone())));
    }
    if entry
        .aliases
        .iter()
        .any(|alias| alias.normalized.starts_with(query))
    {
        return Some((
            MatchKind::AliasPrefix,
            matching(&entry.aliases, query, Mode::Prefix),
        ));
    }
    if let Some(value) = matching(&entry.aliases, query, Mode::Contains) {
        return Some((MatchKind::AliasContains, Some(value)));
    }
    let pinyin_needles = pinyin_needles(query);
    if let Some(value) = matching_any(&entry.pinyin, &pinyin_needles, Mode::Exact) {
        return Some((MatchKind::PinyinExact, Some(value)));
    }
    if let Some(value) = matching_any(&entry.pinyin, &pinyin_needles, Mode::Prefix) {
        return Some((MatchKind::PinyinPrefix, Some(value)));
    }
    if let Some(value) = matching_any(&entry.pinyin, &pinyin_needles, Mode::Contains) {
        return Some((MatchKind::PinyinContains, Some(value)));
    }
    matching(&entry.keywords, query, Mode::Contains)
        .map(|value| (MatchKind::KeywordContains, Some(value)))
}

#[derive(Clone, Copy)]
enum Mode {
    Exact,
    Prefix,
    Contains,
}

fn matching(values: &[SearchTerm], query: &str, mode: Mode) -> Option<String> {
    values
        .iter()
        .find(|value| match mode {
            Mode::Exact => value.normalized == query,
            Mode::Prefix => value.normalized.starts_with(query),
            Mode::Contains => value.normalized.contains(query),
        })
        .map(|value| value.display.clone())
}

fn matching_any(values: &[SearchTerm], queries: &[String], mode: Mode) -> Option<String> {
    for query in queries {
        let selected = match mode {
            Mode::Exact => matching(values, query, Mode::Exact),
            Mode::Prefix => matching(values, query, Mode::Prefix),
            Mode::Contains => matching(values, query, Mode::Contains),
        };
        if selected.is_some() {
            return selected;
        }
    }
    None
}

fn pinyin_needles(query: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut initials = String::new();
    for character in query.chars() {
        if let Some(pinyin) = character.to_pinyin() {
            let plain = pinyin.plain().to_owned();
            if let Some(initial) = plain.chars().next() {
                initials.push(initial);
            }
            parts.push(plain);
        } else if character.is_ascii_alphanumeric() {
            let value = character.to_ascii_lowercase();
            initials.push(value);
            parts.push(value.to_string());
        }
    }
    let mut result = vec![query.to_owned()];
    if !parts.is_empty() {
        let compact = parts.join("");
        if !result.contains(&compact) {
            result.push(compact);
        }
        let spaced = parts.join(" ");
        if !result.contains(&spaced) {
            result.push(spaced);
        }
    }
    if !initials.is_empty() && !result.contains(&initials) {
        result.push(initials);
    }
    result
}
