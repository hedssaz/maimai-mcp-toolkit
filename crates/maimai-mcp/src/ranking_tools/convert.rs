use std::{str::FromStr, sync::Arc, time::Duration};

use maimai_app::{
    rankings::{
        B50ReportOptions, B50Sort, ContextSize, MaxConcurrency, MaxMembers, OutputMode, QueryDelay,
        RankWindow, RefreshOptions, SongReportOptions, SongSort, SortOrder,
    },
    scores::ExactRatio,
};
use maimai_core::{AchievementRate, Difficulty, GroupId, QqId};
use maimai_providers::{NapCatClient, NapCatConfig};
use rust_decimal::Decimal;
use serde_json::Value;

use super::{dto::RefreshArgs, error::RankingToolError};

pub fn group_id(value: Option<String>) -> Result<GroupId, RankingToolError> {
    GroupId::new(value.unwrap_or_default())
        .map_err(|_| RankingToolError::invalid("必须提供 groupId。"))
}

pub fn optional_group_id(value: Option<String>) -> Result<Option<GroupId>, RankingToolError> {
    value
        .map(GroupId::new)
        .transpose()
        .map_err(|_| RankingToolError::invalid("groupId 格式不正确。"))
}

pub fn optional_qq(value: Option<String>) -> Result<Option<QqId>, RankingToolError> {
    value
        .map(QqId::new)
        .transpose()
        .map_err(|_| RankingToolError::invalid("qq 格式不正确。"))
}

pub fn context(value: Option<u8>) -> Result<ContextSize, RankingToolError> {
    ContextSize::new(value.unwrap_or(3)).map_err(RankingToolError::from)
}

pub fn refresh(value: &RefreshArgs) -> Result<RefreshOptions, RankingToolError> {
    if value
        .batch_size
        .is_some_and(|batch_size| !(1..=100).contains(&batch_size))
    {
        return Err(RankingToolError::invalid("batchSize 必须在 1 到 100。"));
    }
    Ok(RefreshOptions {
        no_cache: value.no_cache.unwrap_or(true),
        query_delay: QueryDelay::new(Duration::from_millis(value.query_delay_ms.unwrap_or(250)))
            .map_err(RankingToolError::from)?,
        max_concurrency: MaxConcurrency::new(value.max_concurrency.unwrap_or(3))
            .map_err(RankingToolError::from)?,
        max_members: value
            .max_members
            .map(MaxMembers::new)
            .transpose()
            .map_err(RankingToolError::from)?,
    })
}

pub fn napcat_client(
    default: &NapCatClient,
    args: &RefreshArgs,
) -> Result<Arc<NapCatClient>, RankingToolError> {
    if args.napcat_base_url.is_none() && args.timeout_ms.is_none() {
        return Ok(Arc::new(default.clone()));
    }
    let base_url = args
        .napcat_base_url
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|_| RankingToolError::invalid("napcatBaseUrl 格式不正确。"))?
        .unwrap_or_else(|| default.config().base_url().clone());
    let timeout_ms = match args.timeout_ms {
        Some(value) => value,
        None => u64::try_from(default.config().timeout().as_millis())
            .map_err(|_| RankingToolError::invalid("NapCat timeout 超出范围。"))?,
    };
    if !(1_000..=60_000).contains(&timeout_ms) {
        return Err(RankingToolError::invalid(
            "timeoutMs 必须在 1000 到 60000。",
        ));
    }
    let timeout = Duration::from_millis(timeout_ms);
    let config = NapCatConfig::new(base_url, timeout, default.config().access_token().cloned())
        .map_err(|_| RankingToolError::invalid("NapCat 配置不正确。"))?;
    NapCatClient::new(config)
        .map(Arc::new)
        .map_err(|_| RankingToolError::invalid("NapCat 配置不正确。"))
}

pub struct B50OptionInput {
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub output_mode: Option<String>,
    pub rating_min: Option<u32>,
    pub rating_max: Option<u32>,
    pub fit_min: Option<Value>,
    pub fit_max: Option<Value>,
    pub window: RankWindow,
    pub default_order: SortOrder,
}

pub fn b50_options(input: B50OptionInput) -> Result<B50ReportOptions, RankingToolError> {
    Ok(B50ReportOptions {
        sort: match input.sort_by.as_deref().unwrap_or("rating") {
            "rating" => B50Sort::Rating,
            "fitIndex" => B50Sort::FitIndex,
            _ => {
                return Err(RankingToolError::invalid(
                    "sortBy 必须是 rating 或 fitIndex。",
                ));
            }
        },
        order: order(input.sort_order, input.default_order)?,
        output: match input.output_mode.as_deref().unwrap_or("rating") {
            "rating" => OutputMode::Rating,
            "detail" => OutputMode::Detail,
            _ => {
                return Err(RankingToolError::invalid(
                    "outputMode 必须是 rating 或 detail。",
                ));
            }
        },
        rating_min: input.rating_min,
        rating_max: input.rating_max,
        fit_min: input.fit_min.map(exact_ratio).transpose()?,
        fit_max: input.fit_max.map(exact_ratio).transpose()?,
        window: input.window.validate().map_err(RankingToolError::from)?,
    })
}

pub fn song_options(
    target: maimai_app::rankings::SongTarget,
    sort_by: Option<String>,
    sort_order: Option<String>,
    achievements_min: Option<Value>,
    achievements_max: Option<Value>,
    window: RankWindow,
) -> Result<SongReportOptions, RankingToolError> {
    Ok(SongReportOptions {
        target,
        sort: match sort_by.as_deref().unwrap_or("achievements") {
            "achievements" => SongSort::Achievements,
            "ra" => SongSort::Rating,
            "dxScore" => SongSort::DxScore,
            _ => {
                return Err(RankingToolError::invalid(
                    "sortBy 必须是 achievements / ra / dxScore。",
                ));
            }
        },
        order: order(sort_order, SortOrder::Descending)?,
        achievements_min: achievements_min.map(achievement).transpose()?,
        achievements_max: achievements_max.map(achievement).transpose()?,
        window: window.validate().map_err(RankingToolError::from)?,
    })
}

pub fn window(
    limit: Option<usize>,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<RankWindow, RankingToolError> {
    RankWindow { limit, start, end }
        .validate()
        .map_err(RankingToolError::from)
}

pub fn difficulty(value: Option<u8>) -> Result<Option<Difficulty>, RankingToolError> {
    value
        .map(|value| match value {
            0 => Ok(Difficulty::Basic),
            1 => Ok(Difficulty::Advanced),
            2 => Ok(Difficulty::Expert),
            3 => Ok(Difficulty::Master),
            4 => Ok(Difficulty::ReMaster),
            _ => Err(RankingToolError::invalid("levelIndex 必须在 0 到 4。")),
        })
        .transpose()
}

pub fn deluxe(value: Option<String>) -> Result<Option<bool>, RankingToolError> {
    value
        .map(|value| match value.as_str() {
            "DX" => Ok(true),
            "SD" | "Standard" => Ok(false),
            _ => Err(RankingToolError::invalid(
                "songType 必须是 DX / SD / Standard。",
            )),
        })
        .transpose()
}

pub fn music_id(value: Option<Value>) -> Result<Option<u32>, RankingToolError> {
    value
        .map(|value| match value {
            Value::Number(value) => value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| RankingToolError::invalid("musicId 必须是正整数。")),
            Value::String(value) => value
                .trim()
                .trim_start_matches(['i', 'I', 'd', 'D'])
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| RankingToolError::invalid("musicId 格式不正确。")),
            _ => Err(RankingToolError::invalid("musicId 格式不正确。")),
        })
        .transpose()
}

fn order(value: Option<String>, default: SortOrder) -> Result<SortOrder, RankingToolError> {
    match value.as_deref() {
        None => Ok(default),
        Some("asc") => Ok(SortOrder::Ascending),
        Some("desc") => Ok(SortOrder::Descending),
        _ => Err(RankingToolError::invalid("sortOrder 必须是 asc 或 desc。")),
    }
}

fn achievement(value: Value) -> Result<AchievementRate, RankingToolError> {
    AchievementRate::from_decimal_str(&number_text(value)?)
        .map_err(|_| RankingToolError::invalid("达成率格式不正确。"))
}

fn exact_ratio(value: Value) -> Result<ExactRatio, RankingToolError> {
    let decimal = Decimal::from_str(&number_text(value)?)
        .map_err(|_| RankingToolError::invalid("fitIndex 范围格式不正确。"))?;
    ExactRatio::new(decimal.mantissa(), 10_u128.pow(decimal.scale()))
        .ok_or_else(|| RankingToolError::invalid("fitIndex 范围超出支持范围。"))
}

fn number_text(value: Value) -> Result<String, RankingToolError> {
    match value {
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(RankingToolError::invalid("必须提供数字。")),
    }
}
