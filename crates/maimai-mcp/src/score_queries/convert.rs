use std::{str::FromStr, time::Duration};

use maimai_app::{
    identity::{IdentityDirectory, IdentityQuery, MaxResults},
    scores::{Lookup, SongFilter, SongGenerationFilter},
};
use maimai_catalog::{CatalogQuery, CatalogSnapshot, SearchHit, SourceKind};
use maimai_core::{
    ChartGeneration, Difficulty, GroupId, PlayerUsername, QqId, ScoreSource, SongIdNamespace,
    SourceSongId,
};
use maimai_providers::{DivingFishCredentials, DivingFishOperation, DivingFishRequest, QueryValue};
use maimai_storage::IdentityRecord;
use secrecy::SecretString;
use serde_json::Value;

use super::{dto::DivingFishApiArgs, error::ScoreQueryToolError};

pub struct LookupInput {
    pub qq: Option<String>,
    pub username: Option<String>,
    pub target: Option<String>,
    pub group_id: Option<String>,
}

pub struct ResolvedLookup {
    pub lookup: Lookup,
    pub identity: Option<IdentityRecord>,
}

pub async fn lookup(
    input: LookupInput,
    identities: &IdentityDirectory,
) -> Result<ResolvedLookup, ScoreQueryToolError> {
    let group_id = input
        .group_id
        .map(GroupId::new)
        .transpose()
        .map_err(|_| ScoreQueryToolError::invalid("groupId 格式不正确。"))?;
    let qq = optional_text(input.qq, "qq")?;
    let username = optional_text(input.username, "username")?;
    let target = optional_text(input.target, "target")?;
    if usize::from(qq.is_some()) + usize::from(username.is_some()) + usize::from(target.is_some())
        != 1
    {
        return Err(ScoreQueryToolError::invalid(
            "qq、username、target 必须且只能提供一个。",
        ));
    }
    if let Some(value) = qq {
        let qq =
            QqId::new(value).map_err(|_| ScoreQueryToolError::invalid("qq 必须是数字字符串。"))?;
        let identity = identities.get_identity(&qq, group_id.as_ref()).await?;
        return Ok(ResolvedLookup {
            lookup: Lookup::Qq(qq),
            identity,
        });
    }
    if let Some(value) = username {
        return Ok(ResolvedLookup {
            lookup: Lookup::Username(
                PlayerUsername::new(value)
                    .map_err(|_| ScoreQueryToolError::invalid("username 格式不正确。"))?,
            ),
            identity: None,
        });
    }
    let target = target.ok_or_else(|| ScoreQueryToolError::invalid("必须提供查询目标。"))?;
    let query = IdentityQuery::new(target.clone())
        .map_err(|_| ScoreQueryToolError::invalid("target 格式不正确。"))?;
    let resolution = identities
        .resolve_identity(
            &query,
            group_id.as_ref(),
            MaxResults::new(20).map_err(|_| ScoreQueryToolError::internal())?,
        )
        .await?;
    if resolution.ambiguous {
        return Err(ScoreQueryToolError::invalid(
            "target 匹配多个 QQ，请提供明确 QQ。",
        ));
    }
    if let Some(candidate) = resolution.matches.into_iter().next() {
        let qq = candidate.identity.qq.clone();
        return Ok(ResolvedLookup {
            lookup: Lookup::Qq(qq),
            identity: Some(candidate.identity),
        });
    }
    Ok(ResolvedLookup {
        lookup: Lookup::Username(
            PlayerUsername::new(target)
                .map_err(|_| ScoreQueryToolError::invalid("target 格式不正确。"))?,
        ),
        identity: None,
    })
}

pub fn source(
    value: Option<String>,
    public: bool,
) -> Result<Option<ScoreSource>, ScoreQueryToolError> {
    if public {
        if value.is_some() {
            return Err(ScoreQueryToolError::invalid(
                "public surface 不接受 source。",
            ));
        }
        return Ok(Some(ScoreSource::DivingFish));
    }
    value
        .map(|value| match value.trim().to_ascii_lowercase().as_str() {
            "sy" | "diving-fish" | "divingfish" | "waterfish" | "水鱼" => {
                Ok(ScoreSource::DivingFish)
            }
            "local" | "cache" | "本地" | "缓存" => Ok(ScoreSource::Local),
            "lxns" | "luoxue" | "落雪" => Ok(ScoreSource::Lxns),
            _ => Err(ScoreQueryToolError::invalid(
                "source 必须是 local、sy 或 lxns。",
            )),
        })
        .transpose()
}

pub fn credentials(
    explicit: Option<String>,
) -> Result<Option<DivingFishCredentials>, ScoreQueryToolError> {
    if let Some(token) = optional_text(explicit, "developerToken")? {
        return Ok(Some(
            DivingFishCredentials::new().with_developer_token(SecretString::from(token)),
        ));
    }
    Ok(None)
}

pub fn timeout(value: Option<u64>) -> Result<Duration, ScoreQueryToolError> {
    let value = value.unwrap_or(10_000);
    if !(1_000..=30_000).contains(&value) {
        return Err(ScoreQueryToolError::invalid(
            "timeoutMs 必须是 1000 到 30000 之间的整数。",
        ));
    }
    Ok(Duration::from_millis(value))
}

pub fn song_filter(
    music_id: Option<Value>,
    song_query: Option<String>,
    difficulty: Option<String>,
    song_type: Option<String>,
    search_limit: Option<usize>,
    catalog: &CatalogSnapshot,
) -> Result<SongFilter, ScoreQueryToolError> {
    let music_id = music_id.map(parse_music_id).transpose()?;
    let song_query = optional_text(song_query, "songQuery")?;
    if music_id.is_some() == song_query.is_some() {
        return Err(ScoreQueryToolError::invalid(
            "musicId 和 songQuery 必须且只能提供一个。",
        ));
    }
    let difficulty = difficulty.map(parse_difficulty).transpose()?;
    let generation = song_type.map(parse_generation).transpose()?;
    let song = if let Some(id) = music_id {
        SourceSongId::numeric(SongIdNamespace::DivingFish, id)
    } else {
        let limit = search_limit.unwrap_or(5);
        if !(1..=20).contains(&limit) {
            return Err(ScoreQueryToolError::invalid(
                "searchLimit 必须是 1 到 20 之间的整数。",
            ));
        }
        let mut query = CatalogQuery::text(song_query.unwrap_or_default(), limit);
        query.difficulties.extend(difficulty);
        if let Some(generation) = generation {
            match generation {
                SongGenerationFilter::Exact(value) => {
                    query.generations.insert(value);
                }
                SongGenerationFilter::UtageAny => {
                    query.generations.extend([
                        ChartGeneration::UtageOnePlayer,
                        ChartGeneration::UtageTwoPlayer,
                    ]);
                }
            }
        }
        let hits = catalog
            .query(&query)
            .map_err(|_| ScoreQueryToolError::invalid("songQuery 曲库查询失败。"))?;
        let hit = hits
            .first()
            .ok_or_else(|| ScoreQueryToolError::invalid("songQuery 没有匹配曲目。"))?;
        SourceSongId::numeric(SongIdNamespace::DivingFish, diving_fish_id(hit)?)
    };
    Ok(SongFilter {
        song,
        generation,
        difficulty,
    })
}

pub fn diving_fish_id(hit: &SearchHit<'_>) -> Result<u32, ScoreQueryToolError> {
    let mut ids = hit
        .metadata
        .source_projections
        .iter()
        .filter(|projection| projection.source == SourceKind::DivingFish)
        .filter_map(|projection| match projection.id.value() {
            maimai_core::SongIdValue::Numeric(value) => Some(*value),
            maimai_core::SongIdValue::Text(_) => None,
        })
        .collect::<Vec<_>>();
    if ids.is_empty() {
        ids.extend(hit.music.source_ids.iter().filter_map(|id| {
            (id.namespace() == SongIdNamespace::DivingFish)
                .then_some(id.value())
                .and_then(|value| match value {
                    maimai_core::SongIdValue::Numeric(value) => Some(*value),
                    maimai_core::SongIdValue::Text(_) => None,
                })
        }));
    }
    ids.sort_unstable();
    ids.dedup();
    match ids.as_slice() {
        [id] => Ok(*id),
        [] => Err(ScoreQueryToolError::invalid(
            "匹配曲目没有 Diving-Fish 数字 ID。",
        )),
        _ => Err(ScoreQueryToolError::invalid(
            "匹配曲目对应多个 Diving-Fish ID。",
        )),
    }
}

pub fn generic_request(
    args: DivingFishApiArgs,
    bound_developer_token: Option<SecretString>,
) -> Result<DivingFishRequest, ScoreQueryToolError> {
    let operation = DivingFishOperation::from_str(args.operation.as_deref().unwrap_or_default())
        .map_err(|_| ScoreQueryToolError::invalid("operation 不受支持。"))?;
    let body = args.body;
    let login = if operation == DivingFishOperation::MaimaiLogin {
        Some(login_credentials(body.as_ref())?)
    } else {
        None
    };
    let mut request = DivingFishRequest::new(operation).with_timeout(timeout(args.timeout_ms)?);
    for (key, value) in args.query.unwrap_or_default() {
        request = request.with_query(key, query_value(value)?);
    }
    if operation != DivingFishOperation::MaimaiLogin
        && let Some(body) = body
    {
        request = request.with_body(body);
    }
    if let Some(raw) = args.raw_body {
        request = request.with_raw_body(raw);
    }
    if let Some(etag) = args.if_none_match {
        request = request
            .try_with_if_none_match(&etag)
            .map_err(ScoreQueryToolError::provider)?;
    }
    if let Some(headers) = args.headers {
        request = request
            .try_with_headers(headers)
            .map_err(ScoreQueryToolError::provider)?;
    }
    if let Some(confirm) = args.confirm {
        request = request.confirm(
            DivingFishOperation::from_str(&confirm)
                .map_err(|_| ScoreQueryToolError::invalid("confirm operation 不受支持。"))?,
        );
    }
    let developer = optional_text(args.developer_token, "developerToken")?
        .map(SecretString::from)
        .or(bound_developer_token);
    let mut credentials = DivingFishCredentials::new();
    if let Some(value) = developer {
        credentials = credentials.with_developer_token(value);
    }
    if let Some(value) = optional_text(args.import_token, "importToken")? {
        credentials = credentials.with_import_token(value);
    }
    if let Some(value) = optional_text(args.jwt_token, "jwtToken")? {
        credentials = credentials.with_jwt_token(value);
    }
    if let Some((username, password)) = login {
        credentials = credentials.with_login(username, password);
    }
    Ok(request.with_credentials(credentials))
}

fn login_credentials(body: Option<&Value>) -> Result<(String, SecretString), ScoreQueryToolError> {
    let object = body.and_then(Value::as_object).ok_or_else(|| {
        ScoreQueryToolError::invalid("maimai_login 需要 body.username 和 body.password。")
    })?;
    let username = required_text_field(object, "username", "body.username")?;
    let password = required_text_field(object, "password", "body.password")?;
    Ok((username, SecretString::from(password)))
}

fn required_text_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
    label: &'static str,
) -> Result<String, ScoreQueryToolError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ScoreQueryToolError::invalid(format!("{label} 不能为空。")))?;
    optional_text(Some(value.to_owned()), label)?
        .ok_or_else(|| ScoreQueryToolError::invalid(format!("{label} 不能为空。")))
}

fn query_value(value: Value) -> Result<QueryValue, ScoreQueryToolError> {
    match value {
        Value::String(value) => Ok(QueryValue::Single(value)),
        Value::Number(value) => Ok(QueryValue::Single(value.to_string())),
        Value::Bool(value) => Ok(QueryValue::Single(value.to_string())),
        Value::Array(values) => values
            .into_iter()
            .map(|value| match value {
                Value::String(value) => Ok(value),
                Value::Number(value) => Ok(value.to_string()),
                Value::Bool(value) => Ok(value.to_string()),
                _ => Err(ScoreQueryToolError::invalid("query 数组只能包含标量。")),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(QueryValue::Multiple),
        _ => Err(ScoreQueryToolError::invalid(
            "query 值必须是标量或标量数组。",
        )),
    }
}

fn parse_music_id(value: Value) -> Result<u32, ScoreQueryToolError> {
    let text = match value {
        Value::Number(value) => value.to_string(),
        Value::String(value) => value,
        _ => return Err(ScoreQueryToolError::invalid("musicId 必须是整数或字符串。")),
    };
    let text = text.trim().strip_prefix("id").unwrap_or(text.trim()).trim();
    let value = text
        .parse::<u32>()
        .map_err(|_| ScoreQueryToolError::invalid("musicId 必须是正整数。"))?;
    if value == 0 {
        return Err(ScoreQueryToolError::invalid("musicId 必须是正整数。"));
    }
    Ok(value)
}

pub fn parse_difficulty(value: String) -> Result<Difficulty, ScoreQueryToolError> {
    match value
        .trim()
        .to_ascii_lowercase()
        .replace([':', '_', '-', ' '], "")
        .as_str()
    {
        "basic" | "绿" | "綠" => Ok(Difficulty::Basic),
        "advanced" | "黄" | "黃" => Ok(Difficulty::Advanced),
        "expert" | "红" | "紅" => Ok(Difficulty::Expert),
        "master" | "紫" => Ok(Difficulty::Master),
        "remaster" | "白" => Ok(Difficulty::ReMaster),
        "utage" | "宴" => Ok(Difficulty::Utage),
        _ => Err(ScoreQueryToolError::invalid("difficulty 格式不正确。")),
    }
}

fn parse_generation(value: String) -> Result<SongGenerationFilter, ScoreQueryToolError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sd" | "st" | "standard" => Ok(SongGenerationFilter::Exact(ChartGeneration::Standard)),
        "dx" | "deluxe" => Ok(SongGenerationFilter::Exact(ChartGeneration::Deluxe)),
        "utage" | "宴" => Ok(SongGenerationFilter::UtageAny),
        _ => Err(ScoreQueryToolError::invalid("songType 格式不正确。")),
    }
}

fn optional_text(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<String>, ScoreQueryToolError> {
    value
        .map(|value| {
            if value.chars().any(char::is_control) {
                return Err(ScoreQueryToolError::invalid(format!(
                    "{field} 不能包含控制字符。"
                )));
            }
            let value = value.trim().to_owned();
            if value.is_empty() {
                return Err(ScoreQueryToolError::invalid(format!("{field} 不能为空。")));
            }
            Ok(value)
        })
        .transpose()
}
