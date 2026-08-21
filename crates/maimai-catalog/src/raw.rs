use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Number;

#[derive(Clone, Debug, Deserialize)]
pub(super) struct LxnsCatalog {
    #[serde(default)]
    pub(super) songs: Vec<LxnsSong>,
    #[serde(default)]
    pub(super) genres: Vec<LxnsGenre>,
    #[serde(default)]
    pub(super) versions: Vec<LxnsVersion>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct LxnsGenre {
    pub(super) title: String,
    pub(super) genre: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct LxnsVersion {
    pub(super) title: String,
    pub(super) version: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct LxnsSong {
    pub(super) id: u32,
    pub(super) title: String,
    #[serde(default)]
    pub(super) artist: String,
    #[serde(default)]
    pub(super) genre: String,
    pub(super) bpm: Number,
    pub(super) version: u32,
    #[serde(default)]
    pub(super) difficulties: LxnsDifficulties,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct LxnsDifficulties {
    #[serde(default)]
    pub(super) standard: Vec<LxnsChart>,
    #[serde(default)]
    pub(super) dx: Vec<LxnsChart>,
    #[serde(default)]
    pub(super) utage: Vec<LxnsChart>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct LxnsChart {
    pub(super) difficulty: u8,
    pub(super) level: String,
    pub(super) level_value: Number,
    #[serde(default)]
    pub(super) note_designer: String,
    pub(super) version: Option<u32>,
    pub(super) kanji: Option<String>,
    pub(super) description: Option<String>,
    #[serde(default)]
    pub(super) is_buddy: bool,
    #[serde(default)]
    pub(super) notes: RawNoteCounts,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub(super) struct RawNoteCounts {
    #[serde(default)]
    pub(super) tap: u32,
    #[serde(default)]
    pub(super) hold: u32,
    #[serde(default)]
    pub(super) slide: u32,
    #[serde(default)]
    pub(super) touch: u32,
    #[serde(default, rename = "break")]
    pub(super) break_notes: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct DivingFishSong {
    pub(super) id: String,
    pub(super) title: String,
    #[serde(rename = "type")]
    pub(super) chart_type: String,
    #[serde(default)]
    pub(super) ds: Vec<Number>,
    #[serde(default)]
    pub(super) level: Vec<String>,
    #[serde(default)]
    pub(super) charts: Vec<DivingFishChart>,
    pub(super) basic_info: DivingFishBasicInfo,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct DivingFishChart {
    #[serde(default)]
    pub(super) notes: Vec<u32>,
    #[serde(default)]
    pub(super) charter: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct DivingFishBasicInfo {
    #[serde(default)]
    pub(super) artist: String,
    #[serde(default)]
    pub(super) genre: String,
    pub(super) bpm: Number,
    #[serde(default, rename = "from")]
    pub(super) version: String,
    #[serde(default)]
    pub(super) is_new: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct AliasDocument {
    #[serde(default)]
    pub(super) aliases: Vec<AliasEntry>,
    #[serde(default)]
    pub(super) content: Vec<YuzuAliasEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct AliasEntry {
    pub(super) song_id: serde_json::Value,
    #[serde(default)]
    pub(super) aliases: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct YuzuAliasEntry {
    #[serde(rename = "SongID")]
    pub(super) song_id: serde_json::Value,
    #[serde(default, rename = "Alias")]
    pub(super) alias: Vec<String>,
}

pub(super) type CustomAliasDocument = BTreeMap<String, Vec<String>>;
pub(super) type CharacterMap = BTreeMap<String, String>;

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct PlateDocument {
    #[serde(default)]
    pub(super) content: BTreeMap<String, Vec<u32>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct DxData {
    #[serde(default)]
    pub(super) songs: Vec<DxSong>,
    #[serde(default)]
    pub(super) versions: Vec<DxVersion>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct DxVersion {
    pub(super) version: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DxSong {
    pub(super) song_id: String,
    pub(super) title: String,
    #[serde(default)]
    pub(super) artist: String,
    #[serde(default)]
    pub(super) category: String,
    pub(super) image_name: Option<String>,
    pub(super) bpm: Option<Number>,
    #[serde(default)]
    pub(super) search_acronyms: Vec<String>,
    #[serde(default)]
    pub(super) is_new: bool,
    #[serde(default)]
    pub(super) is_locked: bool,
    #[serde(default)]
    pub(super) sheets: Vec<DxSheet>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DxSheet {
    #[serde(rename = "type")]
    pub(super) chart_type: String,
    pub(super) difficulty: String,
    pub(super) level: String,
    pub(super) internal_level_value: Option<Number>,
    #[serde(default)]
    pub(super) note_designer: Option<String>,
    #[serde(default)]
    pub(super) note_counts: DxNoteCounts,
    #[serde(default)]
    pub(super) regions: DxRegions,
    #[serde(default)]
    pub(super) region_overrides: BTreeMap<String, DxRegionOverride>,
    #[serde(default)]
    pub(super) is_special: bool,
    #[serde(default)]
    pub(super) version: String,
    pub(super) internal_id: Option<u32>,
    pub(super) release_date: Option<String>,
    #[serde(default)]
    pub(super) multiver_internal_level_value: BTreeMap<String, Option<Number>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct DxNoteCounts {
    pub(super) tap: Option<u32>,
    pub(super) hold: Option<u32>,
    pub(super) slide: Option<u32>,
    pub(super) touch: Option<u32>,
    #[serde(rename = "break")]
    pub(super) break_notes: Option<u32>,
    pub(super) total: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DxRegionOverride {
    pub(super) level: Option<String>,
    pub(super) level_value: Option<Number>,
    pub(super) version: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub(super) struct DxRegions {
    #[serde(default)]
    pub(super) jp: bool,
    #[serde(default)]
    pub(super) intl: bool,
    #[serde(default)]
    pub(super) usa: bool,
    #[serde(default)]
    pub(super) cn: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ChartStatsDocument {
    #[serde(default)]
    pub(super) charts: BTreeMap<String, Vec<ChartStat>>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ChartStat {
    pub(super) fit_diff: Option<Number>,
    pub(super) cnt: Option<Number>,
    pub(super) diff: Option<String>,
    pub(super) avg: Option<Number>,
    pub(super) avg_dx: Option<Number>,
    pub(super) std_dev: Option<Number>,
    #[serde(default)]
    pub(super) dist: Vec<Number>,
    #[serde(default)]
    pub(super) fc_dist: Vec<Number>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TagDocument {
    #[serde(default)]
    pub(super) tags: Vec<TagDefinition>,
    #[serde(default)]
    pub(super) tag_songs: Vec<TagSong>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct TagDefinition {
    pub(super) id: u32,
    #[serde(default)]
    pub(super) localized_name: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct TagSong {
    pub(super) song_id: String,
    pub(super) sheet_type: String,
    pub(super) sheet_difficulty: String,
    pub(super) tag_id: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct OfficialDocument {
    #[serde(default)]
    pub(super) songs: Vec<OfficialSong>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OfficialSong {
    pub(super) id: u32,
    pub(super) title: String,
    #[serde(default)]
    pub(super) artist: String,
    #[serde(default)]
    pub(super) genre: String,
    #[serde(default)]
    pub(super) category: String,
    pub(super) bpm: Option<Number>,
    #[serde(default)]
    pub(super) version: String,
    pub(super) release_version: Option<u32>,
    pub(super) official_add_version: Option<String>,
    pub(super) asset_dir: Option<String>,
    pub(super) jacket_path: Option<String>,
    pub(super) cover_id: Option<u32>,
    #[serde(default)]
    pub(super) sheets: Vec<OfficialSheet>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OfficialSheet {
    #[serde(rename = "type")]
    pub(super) chart_type: String,
    pub(super) difficulty: String,
    pub(super) level: String,
    pub(super) internal_level_value: Option<Number>,
    pub(super) note_designer: Option<String>,
    #[serde(default)]
    pub(super) note_counts: DxNoteCounts,
    #[serde(default)]
    pub(super) regions: DxRegions,
    pub(super) internal_id: Option<u32>,
    #[serde(default)]
    pub(super) version: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct DxRatingAlias {
    pub(super) song_id: String,
    pub(super) name: String,
}

pub(super) type NameAliasDocument = BTreeMap<String, Vec<String>>;
