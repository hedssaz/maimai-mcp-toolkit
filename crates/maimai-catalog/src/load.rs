use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[path = "load/aliases.rs"]
mod aliases;
#[path = "load/current.rs"]
mod current;
#[path = "load/diving_fish.rs"]
mod diving_fish;
#[path = "load/documents.rs"]
mod documents;
#[path = "load/error.rs"]
mod error;
#[path = "load/lxns.rs"]
mod lxns;
#[path = "load/numeric.rs"]
mod numeric;
use aliases::{
    AliasIndexes, attach_alias_entries, attach_custom_aliases, attach_dxrating_aliases,
    attach_legacy_aliases, attach_yuzu_aliases, build_name_aliases, collect_hidden_aliases,
    latest_cn_versions,
};
use current::current_diving_fish_versions;
use diving_fish::load_diving_fish_songs;
use documents::{
    CatalogDocuments, build_character_map, parse_json, parse_optional_json, read_document,
    read_optional_document,
};
pub use error::CatalogError;
use lxns::load_lxns_songs;

#[cfg(test)]
#[path = "load/tests.rs"]
mod tests;

use maimai_core::{ChartGeneration, ChartKey, Music, SongIdValue};

use crate::{
    enrich::enrich_catalog,
    metadata::CatalogMetadata,
    normalize::TextNormalizer,
    projection::{ProjectionSources, attach_source_projections},
    raw::{
        AliasDocument, CharacterMap, ChartStatsDocument, CustomAliasDocument, DivingFishSong,
        DxData, DxRatingAlias, LxnsCatalog, NameAliasDocument, OfficialDocument, TagDocument,
    },
    search::SearchIndex,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogFiles {
    pub lxns_song_list: PathBuf,
    pub diving_fish_song_list: PathBuf,
    pub lxns_alias_list: PathBuf,
    pub yuzu_alias_list: PathBuf,
    pub custom_aliases: PathBuf,
    pub pinyin_aliases: PathBuf,
    pub simplified_to_traditional: PathBuf,
    pub dxdata: Option<PathBuf>,
    pub chart_stats: Option<PathBuf>,
    pub tags: Option<PathBuf>,
    pub official_music_data: Option<PathBuf>,
    pub dxrating_aliases: Option<PathBuf>,
    pub legacy_aliases_csv: Option<PathBuf>,
    pub artist_aliases: Option<PathBuf>,
    pub charter_aliases: Option<PathBuf>,
    pub traditional_to_simplified: Option<PathBuf>,
    pub maimaidxplate: Option<PathBuf>,
    pub custom_plates: Option<PathBuf>,
}

impl CatalogFiles {
    pub fn from_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self::main_from_data_dir(data_dir)
    }

    pub fn main_from_data_dir(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref();
        Self {
            lxns_song_list: data_dir.join("lxns_song_list.json"),
            diving_fish_song_list: data_dir.join("divingfish_song_list.json"),
            lxns_alias_list: data_dir.join("lxns_alias_list.json"),
            yuzu_alias_list: data_dir.join("music_alias.json"),
            custom_aliases: data_dir.join("custom_aliases.json"),
            pinyin_aliases: data_dir.join("pinyin_aliases.json"),
            simplified_to_traditional: data_dir.join("zh_s2t.json"),
            dxdata: Some(data_dir.join("dxdata.json")),
            chart_stats: Some(data_dir.join("divingfish_chart_stats.json")),
            tags: Some(data_dir.join("dxrating_tags.json")),
            official_music_data: Some(data_dir.join("official_music_data.json")),
            dxrating_aliases: Some(data_dir.join("dxrating_aliases.json")),
            legacy_aliases_csv: Some(data_dir.join("aliases.csv")),
            artist_aliases: Some(data_dir.join("artist_aliases.json")),
            charter_aliases: Some(data_dir.join("charter_aliases.json")),
            traditional_to_simplified: Some(data_dir.join("zh_t2s.json")),
            maimaidxplate: Some(data_dir.join("maimaidxplate.json")),
            custom_plates: Some(data_dir.join("custom_plates.json")),
        }
    }

    /// Public inputs keep main-only sources disabled even when the directory contains them.
    pub fn public_from_data_dir(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref();
        Self {
            lxns_song_list: data_dir.join("lxns_song_list.json"),
            diving_fish_song_list: data_dir.join("divingfish_song_list.json"),
            lxns_alias_list: data_dir.join("lxns_alias_list.json"),
            yuzu_alias_list: data_dir.join("music_alias.json"),
            custom_aliases: data_dir.join("custom_aliases.json"),
            pinyin_aliases: data_dir.join("pinyin_aliases.json"),
            simplified_to_traditional: data_dir.join("zh_s2t.json"),
            dxdata: None,
            chart_stats: Some(data_dir.join("divingfish_chart_stats.json")),
            tags: None,
            official_music_data: None,
            dxrating_aliases: None,
            legacy_aliases_csv: Some(data_dir.join("aliases.csv")),
            artist_aliases: Some(data_dir.join("artist_aliases.json")),
            charter_aliases: Some(data_dir.join("charter_aliases.json")),
            traditional_to_simplified: Some(data_dir.join("zh_t2s.json")),
            maimaidxplate: Some(data_dir.join("maimaidxplate.json")),
            custom_plates: Some(data_dir.join("custom_plates.json")),
        }
    }

    pub fn load(&self) -> Result<CatalogSnapshot, CatalogError> {
        CatalogSnapshot::load(self)
    }
}

#[derive(Clone, Debug)]
pub struct CatalogSnapshot {
    songs: Vec<Music>,
    pub(crate) chart_index: HashMap<ChartKey, (usize, usize)>,
    normalizer: TextNormalizer,
    search_index: SearchIndex,
    metadata: CatalogMetadata,
    plates: crate::plate::PlateCatalog,
}

impl CatalogSnapshot {
    pub fn load(files: &CatalogFiles) -> Result<Self, CatalogError> {
        let lxns_songs = read_document(&files.lxns_song_list)?;
        let diving_fish_songs = read_document(&files.diving_fish_song_list)?;
        let lxns_aliases = read_document(&files.lxns_alias_list)?;
        let yuzu_aliases = read_document(&files.yuzu_alias_list)?;
        let custom_aliases = read_document(&files.custom_aliases)?;
        let pinyin_aliases = read_document(&files.pinyin_aliases)?;
        let simplified_to_traditional = read_document(&files.simplified_to_traditional)?;
        let dxdata = read_optional_document(files.dxdata.as_deref())?;
        let chart_stats = read_optional_document(files.chart_stats.as_deref())?;
        let tags = read_optional_document(files.tags.as_deref())?;
        let official_music_data = read_optional_document(files.official_music_data.as_deref())?;
        let dxrating_aliases = read_optional_document(files.dxrating_aliases.as_deref())?;
        let legacy_aliases_csv = read_optional_document(files.legacy_aliases_csv.as_deref())?;
        let artist_aliases = read_optional_document(files.artist_aliases.as_deref())?;
        let charter_aliases = read_optional_document(files.charter_aliases.as_deref())?;
        let traditional_to_simplified =
            read_optional_document(files.traditional_to_simplified.as_deref())?;
        let maimaidxplate = read_optional_document(files.maimaidxplate.as_deref())?;
        let custom_plates = read_optional_document(files.custom_plates.as_deref())?;
        let documents = CatalogDocuments {
            lxns_songs: &lxns_songs,
            diving_fish_songs: &diving_fish_songs,
            lxns_aliases: &lxns_aliases,
            yuzu_aliases: &yuzu_aliases,
            custom_aliases: &custom_aliases,
            pinyin_aliases: &pinyin_aliases,
            simplified_to_traditional: &simplified_to_traditional,
            dxdata: dxdata.as_deref(),
            chart_stats: chart_stats.as_deref(),
            tags: tags.as_deref(),
            official_music_data: official_music_data.as_deref(),
            dxrating_aliases: dxrating_aliases.as_deref(),
            legacy_aliases_csv: legacy_aliases_csv.as_deref(),
            artist_aliases: artist_aliases.as_deref(),
            charter_aliases: charter_aliases.as_deref(),
            traditional_to_simplified: traditional_to_simplified.as_deref(),
            maimaidxplate: maimaidxplate.as_deref(),
            custom_plates: custom_plates.as_deref(),
        };
        Self::from_documents(&documents)
    }

    pub fn songs(&self) -> &[Music] {
        &self.songs
    }

    pub(crate) fn normalizer(&self) -> &TextNormalizer {
        &self.normalizer
    }

    pub(crate) fn search_index(&self) -> &SearchIndex {
        &self.search_index
    }

    pub(crate) fn metadata(&self) -> &CatalogMetadata {
        &self.metadata
    }

    pub fn plate_membership(
        &self,
        query: &crate::plate::PlateQuery,
    ) -> crate::plate::PlateMembership {
        self.plates.membership(query, &self.normalizer)
    }

    pub fn plate_members(&self, query: &crate::plate::PlateQuery) -> crate::plate::PlateMembers {
        self.plates.members(query)
    }

    pub fn plate_exists(
        &self,
        name: &crate::plate::PlateName,
        server: crate::plate::PlateServer,
    ) -> bool {
        self.plates.exists(name, server)
    }

    fn from_documents(documents: &CatalogDocuments<'_>) -> Result<Self, CatalogError> {
        let lxns: LxnsCatalog = parse_json("LXNS 曲库", documents.lxns_songs)?;
        let latest_cn_version = lxns
            .versions
            .iter()
            .max_by_key(|version| version.version)
            .map(|version| version.title.clone());
        let diving_fish: Vec<DivingFishSong> =
            parse_json("Diving-Fish 曲库", documents.diving_fish_songs)?;
        let lxns_aliases: AliasDocument = parse_json("LXNS 别名", documents.lxns_aliases)?;
        let yuzu_aliases: AliasDocument = parse_json("Yuzu 别名", documents.yuzu_aliases)?;
        let custom_aliases: CustomAliasDocument =
            parse_json("自定义别名", documents.custom_aliases)?;
        let pinyin_aliases: AliasDocument = parse_json("拼音别名", documents.pinyin_aliases)?;
        let character_map: CharacterMap =
            parse_json("简繁转换表", documents.simplified_to_traditional)?;
        let dxdata: DxData = parse_optional_json("dxdata", documents.dxdata)?;
        let chart_stats: ChartStatsDocument =
            parse_optional_json("Diving-Fish 谱面统计", documents.chart_stats)?;
        let tags: TagDocument = parse_optional_json("DXRating 标签", documents.tags)?;
        let official: OfficialDocument =
            parse_optional_json("官方曲库", documents.official_music_data)?;
        let dxrating_aliases: Vec<DxRatingAlias> =
            parse_optional_json("DXRating 别名", documents.dxrating_aliases)?;
        let artist_aliases: NameAliasDocument =
            parse_optional_json("曲师别名", documents.artist_aliases)?;
        let charter_aliases: NameAliasDocument =
            parse_optional_json("谱师别名", documents.charter_aliases)?;
        let traditional_to_simplified: CharacterMap =
            parse_optional_json("繁简转换表", documents.traditional_to_simplified)?;
        let plate_document: crate::raw::PlateDocument =
            parse_optional_json("国服牌子曲目白名单", documents.maimaidxplate)?;
        let custom_plates = crate::plate::parse_custom_plates(documents.custom_plates)?;

        let normalizer = TextNormalizer::new(build_character_map(character_map)?);
        let mut songs = Vec::new();
        let mut merge_ids = HashMap::new();
        let mut title_index = HashMap::new();
        load_lxns_songs(
            lxns.clone(),
            &normalizer,
            &mut songs,
            &mut merge_ids,
            &mut title_index,
        )?;
        load_diving_fish_songs(
            diving_fish.clone(),
            &normalizer,
            &mut songs,
            &mut merge_ids,
            &mut title_index,
        )?;

        let mut metadata = enrich_catalog(
            &mut songs,
            &normalizer,
            &dxdata,
            &chart_stats,
            &tags,
            latest_cn_version.as_deref(),
        )?;
        let aliases = AliasIndexes::new(&songs, &normalizer);
        attach_alias_entries(&mut songs, &normalizer, &aliases, lxns_aliases.aliases);
        attach_yuzu_aliases(&mut songs, &normalizer, &aliases, yuzu_aliases.content);
        attach_custom_aliases(&mut songs, &normalizer, &aliases, custom_aliases);
        attach_dxrating_aliases(&mut songs, &normalizer, &aliases, dxrating_aliases);
        if let Some(source) = documents.legacy_aliases_csv {
            attach_legacy_aliases(&mut songs, &normalizer, &aliases, source)?;
        }
        let hidden_pinyin =
            collect_hidden_aliases(songs.len(), &normalizer, &aliases, pinyin_aliases.aliases);
        metadata.artist_aliases = build_name_aliases(artist_aliases, &normalizer);
        metadata.charter_aliases = build_name_aliases(charter_aliases, &normalizer);
        metadata.traditional_to_simplified = build_character_map(traditional_to_simplified)?;
        metadata.latest_cn_versions = latest_cn_versions(&lxns);
        metadata.current_diving_fish_versions =
            current_diving_fish_versions(&diving_fish, &metadata.version_order);
        attach_source_projections(
            &songs,
            &normalizer,
            ProjectionSources {
                lxns: &lxns,
                diving_fish: &diving_fish,
                dxdata: &dxdata,
                official: &official,
            },
            &chart_stats,
            &mut metadata,
        )?;
        let plates = crate::plate::PlateCatalog::build(
            plate_document,
            custom_plates,
            &dxdata,
            &songs,
            &metadata,
            &normalizer,
        )?;
        for (song, song_metadata) in songs.iter().zip(&mut metadata.songs) {
            for source_id in &song.source_ids {
                if let SongIdValue::Numeric(value) = source_id.value() {
                    song_metadata.canonical_numeric_ids.insert(*value);
                }
            }
            for source in &song_metadata.source_projections {
                let SongIdValue::Numeric(value) = source.id.value() else {
                    continue;
                };
                song_metadata.canonical_numeric_ids.insert(*value);
                if source.source == crate::metadata::SourceKind::DivingFish
                    && source
                        .charts
                        .iter()
                        .any(|chart| chart.generation == ChartGeneration::Deluxe)
                    && (10_000..100_000).contains(value)
                {
                    song_metadata.canonical_numeric_ids.insert(*value - 10_000);
                }
            }
        }
        let hidden_keywords = metadata
            .songs
            .iter()
            .map(|song| {
                song.source_projections
                    .iter()
                    .flat_map(|source| source.source_fields.search_acronyms.iter())
                    .map(|value| normalizer.normalize(value))
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let search_index = SearchIndex::build(
            &songs,
            &normalizer,
            &hidden_pinyin,
            &hidden_keywords,
            &metadata
                .songs
                .iter()
                .map(|song| song.canonical_numeric_ids.clone())
                .collect::<Vec<_>>(),
        );
        Ok(Self {
            chart_index: crate::lookup::build_chart_index(&songs, &metadata)?,
            songs,
            normalizer,
            search_index,
            metadata,
            plates,
        })
    }
}
