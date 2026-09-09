use std::path::{Path, PathBuf};

use image::RgbaImage;
use maimai_core::SongIdValue;

use crate::{MissingCoverReason, RenderError, ScoreCard, assets::load_cover};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverResolver {
    static_root: PathBuf,
    cache_root: PathBuf,
}

pub(crate) enum ResolvedCover {
    Image(RgbaImage),
    Missing(MissingCoverReason),
}

impl CoverResolver {
    pub fn new(static_root: impl Into<PathBuf>, cache_root: impl Into<PathBuf>) -> Self {
        Self {
            static_root: static_root.into(),
            cache_root: cache_root.into(),
        }
    }

    pub fn static_root(&self) -> &Path {
        &self.static_root
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    pub(crate) fn resolve(&self, card: &ScoreCard) -> Result<ResolvedCover, RenderError> {
        if let Some(path) = card.cover_path() {
            return Ok(match load_cover(path)? {
                Some(image) => ResolvedCover::Image(image),
                None => ResolvedCover::Missing(MissingCoverReason::Unreadable),
            });
        }
        let Some(id) = numeric_id(card) else {
            return Ok(ResolvedCover::Missing(MissingCoverReason::NotProvided));
        };
        let mut unreadable = false;
        for candidate in static_candidates(id)
            .into_iter()
            .map(|name| self.static_root.join("mai/cover").join(name))
            .chain(std::iter::once(
                self.cache_root.join(format!("{}.png", cache_id(id))),
            ))
        {
            if !candidate.is_file() {
                continue;
            }
            match load_cover(&candidate)? {
                Some(image) => return Ok(ResolvedCover::Image(image)),
                None => unreadable = true,
            }
        }
        Ok(ResolvedCover::Missing(if unreadable {
            MissingCoverReason::Unreadable
        } else {
            MissingCoverReason::NotProvided
        }))
    }

    pub(crate) fn resolve_music(
        &self,
        id: Option<&SongIdValue>,
        image_name: Option<&str>,
    ) -> Result<ResolvedCover, RenderError> {
        let mut names = Vec::new();
        if let Some(SongIdValue::Numeric(id)) = id {
            names.extend(static_candidates(*id));
        }
        if let Some(image_name) = image_name.and_then(safe_image_stem) {
            push_unique(&mut names, format!("{image_name}.png"));
            push_unique(&mut names, format!("dxrating_{image_name}.png"));
        }
        let mut unreadable = false;
        for candidate in names.into_iter().flat_map(|name| {
            [
                self.static_root.join("mai/cover").join(&name),
                self.cache_root.join(name),
            ]
        }) {
            if !candidate.is_file() {
                continue;
            }
            match load_cover(&candidate)? {
                Some(image) => return Ok(ResolvedCover::Image(image)),
                None => unreadable = true,
            }
        }
        Ok(ResolvedCover::Missing(if unreadable {
            MissingCoverReason::Unreadable
        } else {
            MissingCoverReason::NotProvided
        }))
    }
}

fn numeric_id(card: &ScoreCard) -> Option<u32> {
    match card.song_id()?.value() {
        SongIdValue::Numeric(value) => Some(*value),
        SongIdValue::Text(value) => value.as_str().parse().ok(),
    }
}

fn static_candidates(id: u32) -> Vec<String> {
    let mut values = vec![id.to_string(), format!("{id:05}")];
    if (1..10_000).contains(&id) {
        let shifted = id + 10_000;
        values.push(shifted.to_string());
        values.push(format!("{shifted:05}"));
    } else if (10_001..100_000).contains(&id) {
        let shifted = id - 10_000;
        values.push(shifted.to_string());
        values.push(format!("{shifted:05}"));
    }
    let mut result = Vec::new();
    for value in values {
        let value = format!("{value}.png");
        if !result.contains(&value) {
            result.push(value);
        }
    }
    result
}

fn cache_id(id: u32) -> String {
    let id = if (10_001..100_000).contains(&id) {
        id - 10_000
    } else {
        id
    };
    format!("{id:05}")
}

fn safe_image_stem(value: &str) -> Option<&str> {
    let name = value.rsplit('/').next()?.rsplit('\\').next()?;
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    (!stem.is_empty()
        && stem.len() <= 255
        && stem
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '.' | '-' | '_')))
    .then_some(stem)
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::{cache_id, static_candidates};

    #[test]
    fn diving_fish_cover_keys_preserve_raw_then_base_id() {
        assert_eq!(
            static_candidates(10_038),
            ["10038.png", "38.png", "00038.png"]
        );
        assert_eq!(cache_id(10_038), "00038");
        assert_eq!(
            static_candidates(11_986),
            ["11986.png", "1986.png", "01986.png"]
        );
        assert_eq!(
            static_candidates(1_310),
            ["1310.png", "01310.png", "11310.png"]
        );
    }
}
