mod builtin;
mod catalog;
mod custom;
mod custom_catalog;
mod member;
mod source_index;

use std::collections::BTreeSet;

use maimai_core::ChartGeneration;
use serde::{Deserialize, Serialize};

use crate::TextNormalizer;

pub(crate) use catalog::PlateCatalog;
pub(crate) use custom::parse as parse_custom_plates;
pub use member::{PlateChart, PlateMember, PlateMemberIdentity, PlateMembers};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlateServer {
    Cn,
    Jp,
    Custom,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PlateName(String);

impl PlateName {
    pub fn new(value: impl Into<String>) -> Result<Self, PlateNameError> {
        let value = value.into();
        if value.chars().any(char::is_control) {
            return Err(PlateNameError);
        }
        let value = value.trim().to_owned();
        if value.is_empty() || value.chars().count() > 64 {
            return Err(PlateNameError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("plate name must be non-empty, at most 64 characters, and contain no control characters")]
pub struct PlateNameError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateQuery {
    name: PlateName,
    server: PlateServer,
}

impl PlateQuery {
    pub const fn new(name: PlateName, server: PlateServer) -> Self {
        Self { name, server }
    }

    pub const fn name(&self) -> &PlateName {
        &self.name
    }

    pub const fn server(&self) -> PlateServer {
        self.server
    }
}

#[derive(Clone)]
pub struct PlateMembership {
    query: PlateQuery,
    song_count: usize,
    numeric_ids: BTreeSet<u32>,
    title_generations: BTreeSet<(String, ChartGeneration)>,
    normalizer: TextNormalizer,
}

impl PlateMembership {
    pub(crate) fn new(
        query: PlateQuery,
        song_count: usize,
        numeric_ids: BTreeSet<u32>,
        title_generations: BTreeSet<(String, ChartGeneration)>,
        normalizer: TextNormalizer,
    ) -> Self {
        Self {
            query,
            song_count,
            numeric_ids,
            title_generations,
            normalizer,
        }
    }

    pub const fn query(&self) -> &PlateQuery {
        &self.query
    }

    pub const fn song_count(&self) -> usize {
        self.song_count
    }

    pub const fn is_empty(&self) -> bool {
        self.song_count == 0
    }

    pub fn matches(
        &self,
        diving_fish_id: Option<u32>,
        title: &str,
        generation: ChartGeneration,
    ) -> bool {
        if diving_fish_id.is_some_and(|id| self.numeric_ids.contains(&id)) {
            return true;
        }
        self.title_generations
            .contains(&(self.normalizer.normalize(title), generation))
    }
}

impl std::fmt::Debug for PlateMembership {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlateMembership")
            .field("query", &self.query)
            .field("song_count", &self.song_count)
            .field("numeric_id_count", &self.numeric_ids.len())
            .field("title_generation_count", &self.title_generations.len())
            .finish()
    }
}
