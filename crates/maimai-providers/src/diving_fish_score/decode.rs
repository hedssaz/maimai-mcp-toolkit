use maimai_core::{
    ChartConstant, Difficulty, FullComboStatus, FullSyncStatus, PlayAchievement,
    PlayAchievementKind, PlayerSelector, PlayerUsername, RatingBreakdown, SongIdNamespace,
    SongIdValue, SourceSongId,
};
use serde_json::{Map, Number, Value};

use super::{
    DivingFishB50, DivingFishChartGeneration, DivingFishPlayer, DivingFishPlayerRecords,
    DivingFishRatingEntry, DivingFishScore, DivingFishScoreCounts, DivingFishScoreError,
};

pub(crate) fn b50(
    value: &Value,
    lookup: PlayerSelector,
) -> Result<DivingFishB50, DivingFishScoreError> {
    let root = object(value, "query_player response")?;
    let charts = object(required(root, &["charts"], "charts")?, "charts")?;
    let sd = scores(required(charts, &["sd"], "charts.sd")?, "charts.sd")?;
    let dx = scores(required(charts, &["dx"], "charts.dx")?, "charts.dx")?;
    let b35 = rating_sum(&sd)?;
    let b15 = rating_sum(&dx)?;
    let total = b35
        .checked_add(b15)
        .ok_or_else(|| invalid("ratingBreakdown 溢出"))?;
    let count = sd
        .len()
        .checked_add(dx.len())
        .ok_or_else(|| invalid("score count 溢出"))?;
    Ok(DivingFishB50 {
        lookup,
        player: player(root)?,
        counts: DivingFishScoreCounts {
            sd: sd.len(),
            dx: dx.len(),
            total: count,
        },
        rating_breakdown: RatingBreakdown { b35, b15, total },
        sd,
        dx,
    })
}

pub(crate) fn records(
    value: &Value,
    lookup: PlayerSelector,
) -> Result<DivingFishPlayerRecords, DivingFishScoreError> {
    let (player, raw_records) = match value {
        Value::Array(values) => (DivingFishPlayer::empty(), values.as_slice()),
        Value::Object(root) => (player(root)?, record_array(root)?),
        _ => return Err(invalid("records response 必须是对象或数组")),
    };
    Ok(DivingFishPlayerRecords {
        lookup,
        player,
        records: raw_records
            .iter()
            .enumerate()
            .map(|(index, value)| score(value, &format!("records[{index}]")))
            .collect::<Result<_, _>>()?,
    })
}

pub(crate) fn ranking(value: &Value) -> Result<Vec<DivingFishRatingEntry>, DivingFishScoreError> {
    let values = value
        .as_array()
        .ok_or_else(|| invalid("rating ranking response 必须是数组"))?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let item = object(value, &format!("ranking[{index}]"))?;
            let username = string(
                required(item, &["username", "name"], "ranking username")?,
                "ranking username",
            )?;
            let username =
                PlayerUsername::new(username).map_err(|_| invalid("ranking username 无效"))?;
            let rating = flexible_u32(
                required(item, &["ra", "rating"], "ranking rating")?,
                "ranking rating",
            )?;
            Ok(DivingFishRatingEntry { username, rating })
        })
        .collect()
}

impl DivingFishPlayer {
    fn empty() -> Self {
        Self {
            nickname: None,
            username: None,
            rating: None,
            additional_rating: None,
            plate: None,
        }
    }
}

fn player(root: &Map<String, Value>) -> Result<DivingFishPlayer, DivingFishScoreError> {
    let username = optional_string(get(root, &["username"]), "player.username")?
        .filter(|value| !value.trim().is_empty())
        .map(PlayerUsername::new)
        .transpose()
        .map_err(|_| invalid("player.username 无效"))?;
    Ok(DivingFishPlayer {
        nickname: optional_string(get(root, &["nickname"]), "player.nickname")?,
        username,
        rating: optional_u32(get(root, &["rating"]), "player.rating")?,
        additional_rating: optional_u32(
            get(root, &["additional_rating", "additionalRating"]),
            "player.additionalRating",
        )?,
        plate: optional_string(get(root, &["plate"]), "player.plate")?,
    })
}

fn scores(value: &Value, field: &str) -> Result<Vec<DivingFishScore>, DivingFishScoreError> {
    value
        .as_array()
        .ok_or_else(|| invalid(format!("{field} 必须是数组")))?
        .iter()
        .enumerate()
        .map(|(index, value)| score(value, &format!("{field}[{index}]")))
        .collect()
}

fn score(value: &Value, field: &str) -> Result<DivingFishScore, DivingFishScoreError> {
    let value = object(value, field)?;
    let declared_generation = generation(required(
        value,
        &["type", "chart_type", "chartType"],
        "score.type",
    )?)?;
    let difficulty = difficulty(value, declared_generation)?;
    let generation = if difficulty == Difficulty::Utage {
        DivingFishChartGeneration::Utage
    } else {
        declared_generation
    };
    let achievement_kind = if difficulty == Difficulty::Utage {
        PlayAchievementKind::Utage
    } else {
        PlayAchievementKind::Ranked
    };
    Ok(DivingFishScore {
        song_id: song_id(required(
            value,
            &["song_id", "songId", "music_id", "musicId", "id"],
            "score.songId",
        )?)?,
        // Titles are display text, not identity. In particular, U+3000 is a real title.
        title: optional_string(get(value, &["title"]), "score.title")?.unwrap_or_default(),
        generation,
        difficulty,
        level: string(required(value, &["level"], "score.level")?, "score.level")?,
        constant: optional_decimal(
            get(value, &["ds", "constant"]),
            "score.ds",
            ChartConstant::from_decimal_str,
        )?,
        achievements: optional_decimal(
            get(value, &["achievements", "achievement"]),
            "score.achievements",
            |value| PlayAchievement::from_decimal_str(achievement_kind, value),
        )?,
        dx_score: optional_u32(get(value, &["dx_score", "dxScore"]), "score.dxScore")?,
        rating: optional_u32(get(value, &["ra", "rating"]), "score.ra")?,
        grade: optional_string(get(value, &["rate", "grade"]), "score.rate")?,
        full_combo: optional_marker::<FullComboStatus>(
            get(value, &["fc", "full_combo", "fullCombo"]),
            "score.fc",
        )?,
        full_sync: optional_marker::<FullSyncStatus>(
            get(value, &["fs", "full_sync", "fullSync"]),
            "score.fs",
        )?,
        version: optional_string(get(value, &["version", "from"]), "score.version")?,
    })
}

fn optional_marker<T>(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<T>, DivingFishScoreError>
where
    T: std::str::FromStr,
{
    optional_string(value, field)?
        // The legacy Diving-Fish renderer treats these as an absent marker.
        .filter(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "" | "none" | "null" | "nan"
            )
        })
        .map(|value| value.parse().map_err(|_| invalid(format!("{field} 无效"))))
        .transpose()
}

fn generation(value: &Value) -> Result<DivingFishChartGeneration, DivingFishScoreError> {
    let value = string(value, "score.type")?.trim().to_ascii_lowercase();
    match value.as_str() {
        "sd" | "st" | "standard" => Ok(DivingFishChartGeneration::Standard),
        "dx" | "deluxe" => Ok(DivingFishChartGeneration::Deluxe),
        "utage" | "宴" => Ok(DivingFishChartGeneration::Utage),
        _ => Err(invalid("score.type 未知")),
    }
}

fn difficulty(
    value: &Map<String, Value>,
    generation: DivingFishChartGeneration,
) -> Result<Difficulty, DivingFishScoreError> {
    let label = optional_string(
        get(value, &["level_label", "levelLabel", "difficulty"]),
        "score.difficulty",
    )?
    .map(|label| {
        label
            .trim()
            .to_ascii_lowercase()
            .replace([':', '-', '_', ' '], "")
    });
    // Historical Utage records use type=DX and an ordinary level_index. The label
    // selects Utage semantics; the catalog later resolves the exact chart identity.
    if generation == DivingFishChartGeneration::Utage
        || matches!(label.as_deref(), Some("utage" | "宴"))
    {
        return Ok(Difficulty::Utage);
    }
    // Ordinary records use the numeric index. The display label is only needed
    // when older responses omit that index.
    match optional_u32(
        get(value, &["level_index", "levelIndex"]),
        "score.levelIndex",
    )? {
        Some(0) => Ok(Difficulty::Basic),
        Some(1) => Ok(Difficulty::Advanced),
        Some(2) => Ok(Difficulty::Expert),
        Some(3) => Ok(Difficulty::Master),
        Some(4) => Ok(Difficulty::ReMaster),
        Some(_) => Err(invalid("score.levelIndex 越界")),
        None => match label.as_deref() {
            Some("basic") => Ok(Difficulty::Basic),
            Some("advanced") => Ok(Difficulty::Advanced),
            Some("expert") => Ok(Difficulty::Expert),
            Some("master") => Ok(Difficulty::Master),
            Some("remaster") => Ok(Difficulty::ReMaster),
            _ => Err(invalid("score.difficulty 缺失或无法解析")),
        },
    }
}

fn song_id(value: &Value) -> Result<SourceSongId, DivingFishScoreError> {
    let value = match value {
        Value::Number(number) => SongIdValue::Numeric(number_u32(number, "score.songId")?),
        Value::String(value) => match value.trim().parse::<u32>() {
            Ok(value) => SongIdValue::Numeric(value),
            Err(_) => SongIdValue::text(value.clone()).map_err(|_| invalid("score.songId 无效"))?,
        },
        _ => return Err(invalid("score.songId 必须是整数或字符串")),
    };
    Ok(SourceSongId::new(SongIdNamespace::DivingFish, value))
}

fn record_array(root: &Map<String, Value>) -> Result<&[Value], DivingFishScoreError> {
    for key in ["records", "verlist"] {
        if let Some(value) = root.get(key) {
            return value
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(|| invalid(format!("{key} 必须是数组")));
        }
    }
    let arrays = root
        .values()
        .filter_map(Value::as_array)
        .collect::<Vec<_>>();
    match arrays.as_slice() {
        [only] => Ok(only.as_slice()),
        _ => Err(invalid("records response 缺少唯一 records/verlist 数组")),
    }
}

fn rating_sum(scores: &[DivingFishScore]) -> Result<u32, DivingFishScoreError> {
    scores.iter().try_fold(0_u32, |sum, score| {
        sum.checked_add(score.rating.unwrap_or(0))
            .ok_or_else(|| invalid("score rating 汇总溢出"))
    })
}

fn optional_decimal<T, E>(
    value: Option<&Value>,
    field: &str,
    parse: impl FnOnce(&str) -> Result<T, E> + Copy,
) -> Result<Option<T>, DivingFishScoreError> {
    value
        .filter(|value| !value.is_null())
        .map(|value| {
            decimal_text(value, field)
                .and_then(|text| parse(&text).map_err(|_| invalid(format!("{field} 无效"))))
        })
        .transpose()
}

fn decimal_text(value: &Value, field: &str) -> Result<String, DivingFishScoreError> {
    match value {
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        _ => Err(invalid(format!("{field} 必须是十进制数或数字字符串"))),
    }
}

fn optional_u32(value: Option<&Value>, field: &str) -> Result<Option<u32>, DivingFishScoreError> {
    value
        .filter(|value| !value.is_null())
        .map(|value| flexible_u32(value, field))
        .transpose()
}

fn flexible_u32(value: &Value, field: &str) -> Result<u32, DivingFishScoreError> {
    match value {
        Value::Number(value) => number_u32(value, field),
        Value::String(value) => value
            .trim()
            .parse()
            .map_err(|_| invalid(format!("{field} 必须是非负整数"))),
        _ => Err(invalid(format!("{field} 必须是非负整数"))),
    }
}

fn number_u32(value: &Number, field: &str) -> Result<u32, DivingFishScoreError> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid(format!("{field} 超出范围或不是整数")))
}

fn optional_string(
    value: Option<&Value>,
    field: &str,
) -> Result<Option<String>, DivingFishScoreError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => string(value, field).map(Some),
    }
}

fn string(value: &Value, field: &str) -> Result<String, DivingFishScoreError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("{field} 必须是字符串")))
}

fn object<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a Map<String, Value>, DivingFishScoreError> {
    value
        .as_object()
        .ok_or_else(|| invalid(format!("{field} 必须是对象")))
}

fn required<'a>(
    root: &'a Map<String, Value>,
    keys: &[&str],
    field: &str,
) -> Result<&'a Value, DivingFishScoreError> {
    get(root, keys)
        .filter(|value| !value.is_null())
        .ok_or_else(|| invalid(format!("{field} 缺失")))
}

fn get<'a>(root: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| root.get(*key))
}

fn invalid(message: impl Into<String>) -> DivingFishScoreError {
    DivingFishScoreError::invalid_response(message)
}
