mod chart;
mod song;

use std::str::FromStr;

use maimai_catalog::{CatalogSnapshot, MatchKind, RegionAvailability, SearchHit, SourceKind};
use maimai_core::{ChartGeneration, Difficulty, SongIdValue, SourceSongId};
use rust_decimal::Decimal;
use serde_json::{Value, json};

pub(super) fn song_value(snapshot: &CatalogSnapshot, hit: &SearchHit<'_>) -> Value {
    song::song_value(snapshot, hit)
}

pub(super) fn source_id_value(id: &SourceSongId) -> String {
    match id.value() {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}

pub(super) const fn chart_type(generation: ChartGeneration) -> &'static str {
    match generation {
        ChartGeneration::Standard => "standard",
        ChartGeneration::Deluxe => "dx",
        ChartGeneration::UtageOnePlayer => "utage1p",
        ChartGeneration::UtageTwoPlayer => "utage2p",
    }
}

pub(super) const fn difficulty(value: Difficulty) -> &'static str {
    match value {
        Difficulty::Basic => "Basic",
        Difficulty::Advanced => "Advanced",
        Difficulty::Expert => "Expert",
        Difficulty::Master => "Master",
        Difficulty::ReMaster => "Re:MASTER",
        Difficulty::Utage => "Utage",
    }
}

pub(super) const fn difficulty_index(value: Difficulty) -> usize {
    match value {
        Difficulty::Basic => 0,
        Difficulty::Advanced => 1,
        Difficulty::Expert => 2,
        Difficulty::Master => 3,
        Difficulty::ReMaster => 4,
        Difficulty::Utage => 0,
    }
}

pub(super) fn regions(value: RegionAvailability) -> Value {
    json!({"jp": value.jp, "intl": value.intl, "usa": value.usa, "cn": value.cn})
}

pub(super) const fn source_priority(value: SourceKind) -> usize {
    match value {
        SourceKind::China => 0,
        SourceKind::Official => 1,
        SourceKind::Japan => 2,
        SourceKind::DivingFish => 3,
    }
}

pub(super) fn match_value(kind: MatchKind, value: &str) -> Value {
    let (field, label, mode) = match kind {
        MatchKind::NumericId => ("song_id", "歌曲ID命中", "exact"),
        MatchKind::ExactTitle => ("title", "歌名命中", "exact"),
        MatchKind::ExactAlias => ("alias", "别名命中", "exact"),
        MatchKind::TitlePrefix => ("title", "歌名前缀", "prefix"),
        MatchKind::TitleContains => ("title", "歌名包含", "contains"),
        MatchKind::AliasPrefix => ("alias", "别名前缀", "prefix"),
        MatchKind::AliasContains => ("alias", "别名包含", "contains"),
        MatchKind::PinyinExact => ("pinyin", "拼音命中", "exact"),
        MatchKind::PinyinPrefix => ("pinyin", "拼音命中", "prefix"),
        MatchKind::PinyinContains => ("pinyin", "拼音命中", "contains"),
        MatchKind::KeywordContains => ("keyword", "关键字命中", "contains"),
        MatchKind::FilterOnly => ("none", "无查询", "none"),
    };
    json!({"field": field, "label": label, "mode": mode, "value": value})
}

pub(super) fn decimal_value(value: Decimal) -> Value {
    match serde_json::Number::from_str(&value.normalize().to_string()) {
        Ok(value) => Value::Number(value),
        Err(_) => Value::Null,
    }
}
