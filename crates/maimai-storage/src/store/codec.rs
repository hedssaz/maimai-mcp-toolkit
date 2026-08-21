use std::str::FromStr;

use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, Difficulty, PlayAchievement,
    PlayAchievementKind, QqId, ScoreSource, SongIdNamespace, SongIdValue,
};
use serde_json::Value;

use crate::StorageError;

pub(crate) fn qq_from_db(value: String) -> Result<QqId, StorageError> {
    QqId::from_str(&value).map_err(|_| StorageError::InvalidStoredValue { field: "qq", value })
}

pub(super) fn encode_optional_json(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<String>, StorageError> {
    value
        .map(|value| {
            serde_json::to_string(value)
                .map_err(|source| StorageError::EncodeJson { field, source })
        })
        .transpose()
}

pub(super) fn decode_optional_json(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<Value>, StorageError> {
    value
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|source| StorageError::StoredJson { field, source })
        })
        .transpose()
}

pub(super) fn require_non_empty(value: &str, field: &'static str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        return Err(StorageError::EmptyField { field });
    }
    Ok(())
}

pub(crate) fn namespace_db(namespace: SongIdNamespace) -> &'static str {
    match namespace {
        SongIdNamespace::DivingFish => "diving_fish",
        SongIdNamespace::Lxns => "lxns",
        SongIdNamespace::OfficialCn => "official_cn",
        SongIdNamespace::DxRating => "dx_rating",
        SongIdNamespace::Yuzu => "yuzu",
    }
}

pub(crate) fn namespace_from_db(value: &str) -> Result<SongIdNamespace, StorageError> {
    match value {
        "diving_fish" => Ok(SongIdNamespace::DivingFish),
        "lxns" => Ok(SongIdNamespace::Lxns),
        "official_cn" => Ok(SongIdNamespace::OfficialCn),
        "dx_rating" => Ok(SongIdNamespace::DxRating),
        "yuzu" => Ok(SongIdNamespace::Yuzu),
        _ => Err(invalid_stored("source_namespace", value)),
    }
}

pub(crate) fn source_value_db(value: &SongIdValue) -> String {
    match value {
        SongIdValue::Numeric(value) => format!("numeric:{value}"),
        SongIdValue::Text(value) => format!("text:{}", value.as_str()),
    }
}

pub(crate) fn source_value_from_db(value: &str) -> Result<SongIdValue, StorageError> {
    if let Some(value) = value.strip_prefix("numeric:") {
        return value
            .parse::<u32>()
            .map(SongIdValue::Numeric)
            .map_err(|_| invalid_stored("source_value", value));
    }
    if let Some(value) = value.strip_prefix("text:") {
        return SongIdValue::text(value.to_owned())
            .map_err(|_| invalid_stored("source_value", value));
    }
    Err(invalid_stored("source_value", value))
}

pub(crate) fn generation_db(generation: ChartGeneration) -> &'static str {
    match generation {
        ChartGeneration::Standard => "standard",
        ChartGeneration::Deluxe => "deluxe",
        ChartGeneration::UtageOnePlayer => "utage_one_player",
        ChartGeneration::UtageTwoPlayer => "utage_two_player",
    }
}

pub(crate) fn generation_from_db(value: &str) -> Result<ChartGeneration, StorageError> {
    match value {
        "standard" => Ok(ChartGeneration::Standard),
        "deluxe" => Ok(ChartGeneration::Deluxe),
        "utage_one_player" => Ok(ChartGeneration::UtageOnePlayer),
        "utage_two_player" => Ok(ChartGeneration::UtageTwoPlayer),
        _ => Err(invalid_stored("generation", value)),
    }
}

pub(crate) fn difficulty_db(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Basic => "basic",
        Difficulty::Advanced => "advanced",
        Difficulty::Expert => "expert",
        Difficulty::Master => "master",
        Difficulty::ReMaster => "re_master",
        Difficulty::Utage => "utage",
    }
}

pub(crate) fn difficulty_from_db(value: &str) -> Result<Difficulty, StorageError> {
    match value {
        "basic" => Ok(Difficulty::Basic),
        "advanced" => Ok(Difficulty::Advanced),
        "expert" => Ok(Difficulty::Expert),
        "master" => Ok(Difficulty::Master),
        "re_master" => Ok(Difficulty::ReMaster),
        "utage" => Ok(Difficulty::Utage),
        _ => Err(invalid_stored("difficulty", value)),
    }
}

pub(crate) fn chart_constant_db(value: ChartConstant) -> String {
    value.value().normalize().to_string()
}

pub(crate) fn chart_constant_from_db(value: &str) -> Result<ChartConstant, StorageError> {
    ChartConstant::from_decimal_str(value).map_err(|_| invalid_stored("ds", value))
}

pub(crate) fn achievement_rate_db(value: AchievementRate) -> String {
    PlayAchievement::from(value).decimal_string()
}

pub(crate) fn achievement_rate_from_db(value: &str) -> Result<AchievementRate, StorageError> {
    AchievementRate::from_decimal_str(value).map_err(|_| invalid_stored("achievements", value))
}

pub(crate) struct DbAchievement {
    pub kind: &'static str,
    pub units: i64,
    pub decimal: String,
}

pub(crate) fn play_achievement_db(
    value: PlayAchievement,
    difficulty: Difficulty,
) -> Result<DbAchievement, StorageError> {
    validate_achievement_kind(value, difficulty)?;
    Ok(DbAchievement {
        kind: value.kind().as_str(),
        units: i64::from(value.ten_thousandths()),
        decimal: value.decimal_string(),
    })
}

pub(crate) fn play_achievement_from_db(
    kind: Option<String>,
    units: Option<i64>,
    difficulty: Difficulty,
) -> Result<Option<PlayAchievement>, StorageError> {
    let (kind, units) = match (kind, units) {
        (None, None) => return Ok(None),
        (Some(kind), Some(units)) => (kind, units),
        (kind, units) => {
            return Err(invalid_stored(
                "player_records_v3.achievement",
                &format!("kind={kind:?},units={units:?}"),
            ));
        }
    };
    let kind = match kind.as_str() {
        "ranked" => PlayAchievementKind::Ranked,
        "utage" => PlayAchievementKind::Utage,
        _ => return Err(invalid_stored("player_records_v3.achievement_kind", &kind)),
    };
    let units = u32::try_from(units)
        .map_err(|_| invalid_stored("player_records_v3.achievement_units", &units.to_string()))?;
    let value = PlayAchievement::from_parts(kind, units)
        .map_err(|_| invalid_stored("player_records_v3.achievement_units", &units.to_string()))?;
    validate_achievement_kind(value, difficulty)?;
    Ok(Some(value))
}

fn validate_achievement_kind(
    value: PlayAchievement,
    difficulty: Difficulty,
) -> Result<(), StorageError> {
    let matches = matches!(
        (value.kind(), difficulty),
        (PlayAchievementKind::Ranked, Difficulty::Basic)
            | (PlayAchievementKind::Ranked, Difficulty::Advanced)
            | (PlayAchievementKind::Ranked, Difficulty::Expert)
            | (PlayAchievementKind::Ranked, Difficulty::Master)
            | (PlayAchievementKind::Ranked, Difficulty::ReMaster)
            | (PlayAchievementKind::Utage, Difficulty::Utage)
    );
    if matches {
        Ok(())
    } else {
        Err(invalid_stored(
            "player_records_v3.achievement_kind",
            value.kind().as_str(),
        ))
    }
}

pub(super) fn score_source_db(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "diving_fish",
        ScoreSource::Lxns => "lxns",
        ScoreSource::Local => "local",
        ScoreSource::OfficialCn => "official_cn",
    }
}

pub(super) fn score_source_from_db(value: &str) -> Result<ScoreSource, StorageError> {
    match value {
        "diving_fish" => Ok(ScoreSource::DivingFish),
        "lxns" => Ok(ScoreSource::Lxns),
        "local" => Ok(ScoreSource::Local),
        "official_cn" => Ok(ScoreSource::OfficialCn),
        _ => Err(invalid_stored("score_source", value)),
    }
}

pub(super) fn score_source_from_legacy_profile(value: &str) -> Option<ScoreSource> {
    let value = value.trim().to_ascii_lowercase();
    if value.starts_with("sdgb_") || value == "official_cn" {
        Some(ScoreSource::OfficialCn)
    } else if matches!(value.as_str(), "diving_fish" | "divingfish") {
        Some(ScoreSource::DivingFish)
    } else if matches!(value.as_str(), "lxns" | "lxns_player") {
        Some(ScoreSource::Lxns)
    } else if value == "local" {
        Some(ScoreSource::Local)
    } else {
        None
    }
}

fn invalid_stored(field: &'static str, value: &str) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.to_owned(),
    }
}
