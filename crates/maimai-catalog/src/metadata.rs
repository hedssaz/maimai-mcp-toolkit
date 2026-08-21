use std::collections::{BTreeMap, BTreeSet, HashMap};

use rust_decimal::Decimal;
use time::Date;

use maimai_core::{ChartGeneration, Difficulty, NoteCounts, SourceSongId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Region {
    Japan,
    International,
    UnitedStates,
    China,
}

impl Region {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Japan => "jp",
            Self::International => "intl",
            Self::UnitedStates => "usa",
            Self::China => "cn",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegionAvailability {
    pub jp: bool,
    pub intl: bool,
    pub usa: bool,
    pub cn: bool,
}

impl RegionAvailability {
    pub const fn has(self, region: Region) -> bool {
        match region {
            Region::Japan => self.jp,
            Region::International => self.intl,
            Region::UnitedStates => self.usa,
            Region::China => self.cn,
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.jp |= other.jp;
        self.intl |= other.intl;
        self.usa |= other.usa;
        self.cn |= other.cn;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SongQueryMetadata {
    pub is_new_cn: bool,
    pub is_new_jp: bool,
    pub is_locked: Option<bool>,
    pub regions: RegionAvailability,
    pub source_labels: BTreeSet<SourceKind>,
    pub source_projections: Vec<SourceSongProjection>,
    pub canonical_numeric_ids: BTreeSet<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChartQueryMetadata {
    pub fit_diff: Option<Decimal>,
    pub version: Option<String>,
    pub release_date: Option<Date>,
    pub regions: RegionAvailability,
    pub tag_ids: BTreeSet<u32>,
    pub multiver_constants: BTreeMap<String, Decimal>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SourceKind {
    China,
    Official,
    Japan,
    DivingFish,
}

impl SourceKind {
    pub const fn key(self) -> &'static str {
        match self {
            Self::China => "cn",
            Self::Official => "official",
            Self::Japan => "jp",
            Self::DivingFish => "divingfish",
        }
    }

    pub const fn source_name(self) -> &'static str {
        match self {
            Self::China => "lxns",
            Self::Official => "official",
            Self::Japan => "dxdata",
            Self::DivingFish => "cndivingfish",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceSongProjection {
    pub source: SourceKind,
    pub id: SourceSongId,
    pub title: String,
    pub artist: String,
    pub genre: String,
    pub bpm: Option<Decimal>,
    pub version: String,
    pub release_date: Option<Date>,
    pub is_new: Option<bool>,
    pub is_locked: Option<bool>,
    pub image_name: Option<String>,
    pub source_fields: SourceSongFields,
    pub charts: Vec<SourceChartProjection>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceSongFields {
    pub numeric_version: Option<u32>,
    pub release_version: Option<u32>,
    pub official_add_version: Option<String>,
    pub category: Option<String>,
    pub asset_dir: Option<String>,
    pub jacket_path: Option<String>,
    pub rights: Option<String>,
    pub map: Option<String>,
    pub slug: Option<String>,
    pub keyword: Option<String>,
    pub comment: Option<String>,
    pub search_acronyms: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceChartProjection {
    pub source_song_id: SourceSongId,
    pub generation: ChartGeneration,
    pub difficulty: Difficulty,
    pub level: String,
    pub constant: Option<Decimal>,
    pub note_designer: String,
    pub notes: Option<NoteCounts>,
    pub note_total: Option<u32>,
    pub version: String,
    pub regions: RegionAvailability,
    pub release_date: Option<Date>,
    pub internal_id: Option<u32>,
    pub fit_source_id: Option<String>,
    pub fit_stats: Option<ChartFitStats>,
    pub music_id: Option<u32>,
    pub chart_id: Option<u32>,
    pub is_buddy: Option<bool>,
    pub kanji: Option<String>,
    pub description: Option<String>,
    pub raw_difficulty: Option<String>,
    pub is_special: bool,
    pub region_overrides: BTreeMap<String, RegionOverride>,
    pub multiver_constants: BTreeMap<String, Decimal>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegionOverride {
    pub level: Option<String>,
    pub constant: Option<Decimal>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartFitStats {
    pub count: Option<Decimal>,
    pub diff: Option<String>,
    pub fit_diff: Option<Decimal>,
    pub average: Option<Decimal>,
    pub average_dx: Option<Decimal>,
    pub standard_deviation: Option<Decimal>,
    pub distribution: Vec<Decimal>,
    pub full_combo_distribution: Vec<Decimal>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CatalogMetadata {
    pub(crate) songs: Vec<SongQueryMetadata>,
    pub(crate) charts: Vec<Vec<ChartQueryMetadata>>,
    pub(crate) tag_names: HashMap<String, u32>,
    pub(crate) tag_labels: HashMap<u32, String>,
    pub(crate) version_order: Vec<String>,
    pub(crate) latest_cn_versions: Vec<u32>,
    pub(crate) current_diving_fish_versions: Vec<String>,
    pub(crate) artist_aliases: HashMap<String, BTreeSet<String>>,
    pub(crate) charter_aliases: HashMap<String, BTreeSet<String>>,
    pub(crate) traditional_to_simplified: HashMap<char, char>,
}
