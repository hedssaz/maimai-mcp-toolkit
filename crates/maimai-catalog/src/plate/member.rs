use maimai_core::{
    ChartConstant, ChartGeneration, ChartKey, Difficulty, SongIdValue, SourceSongId,
};

use super::PlateQuery;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlateMemberIdentity {
    Catalog {
        canonical_song: SourceSongId,
        display_id: SongIdValue,
        diving_fish_id: Option<u32>,
    },
    External {
        display_id: SongIdValue,
    },
}

impl PlateMemberIdentity {
    pub fn canonical_song(&self) -> Option<&SourceSongId> {
        match self {
            Self::Catalog { canonical_song, .. } => Some(canonical_song),
            Self::External { .. } => None,
        }
    }

    pub const fn diving_fish_id(&self) -> Option<u32> {
        match self {
            Self::Catalog { diving_fish_id, .. } => *diving_fish_id,
            Self::External { .. } => None,
        }
    }

    pub const fn display_id(&self) -> &SongIdValue {
        match self {
            Self::Catalog { display_id, .. } | Self::External { display_id } => display_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateChart {
    key: Option<ChartKey>,
    difficulty: Difficulty,
    level: String,
    constant: Option<ChartConstant>,
}

impl PlateChart {
    pub(crate) fn new(
        key: Option<ChartKey>,
        difficulty: Difficulty,
        level: String,
        constant: Option<ChartConstant>,
    ) -> Self {
        Self {
            key,
            difficulty,
            level,
            constant,
        }
    }

    pub fn key(&self) -> Option<&ChartKey> {
        self.key.as_ref()
    }

    pub const fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    pub fn level(&self) -> &str {
        &self.level
    }

    pub const fn constant(&self) -> Option<ChartConstant> {
        self.constant
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateMember {
    identity: PlateMemberIdentity,
    title: String,
    generation: ChartGeneration,
    image_name: Option<String>,
    charts: Vec<PlateChart>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlateMembers {
    query: PlateQuery,
    declared_song_count: usize,
    members: Vec<PlateMember>,
}

impl PlateMembers {
    pub(crate) fn new(
        query: PlateQuery,
        declared_song_count: usize,
        members: Vec<PlateMember>,
    ) -> Self {
        Self {
            query,
            declared_song_count,
            members,
        }
    }

    pub const fn query(&self) -> &PlateQuery {
        &self.query
    }

    pub const fn declared_song_count(&self) -> usize {
        self.declared_song_count
    }

    pub fn members(&self) -> &[PlateMember] {
        &self.members
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

impl PlateMember {
    pub(crate) fn new(
        identity: PlateMemberIdentity,
        title: String,
        generation: ChartGeneration,
        image_name: Option<String>,
        charts: Vec<PlateChart>,
    ) -> Self {
        Self {
            identity,
            title,
            generation,
            image_name,
            charts,
        }
    }

    pub const fn identity(&self) -> &PlateMemberIdentity {
        &self.identity
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub const fn generation(&self) -> ChartGeneration {
        self.generation
    }

    pub fn image_name(&self) -> Option<&str> {
        self.image_name.as_deref()
    }

    pub fn charts(&self) -> &[PlateChart] {
        &self.charts
    }

    pub fn master_constant(&self) -> Option<ChartConstant> {
        self.charts
            .iter()
            .find(|chart| chart.difficulty == Difficulty::Master)
            .and_then(PlateChart::constant)
    }
}
