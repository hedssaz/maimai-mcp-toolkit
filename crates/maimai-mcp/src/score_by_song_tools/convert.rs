use std::time::Duration;

use maimai_app::{
    score_by_song::{PlayerLookupRequest, ScoreBySongRequest, SongLookupRequest},
    scores::SongGenerationFilter,
};
use maimai_core::{ChartGeneration, Difficulty, GroupId, PlayerUsername, QqId};
use time::OffsetDateTime;

use super::{dto::QueryScoreBySongArgs, error::ScoreBySongToolError};

pub struct ConvertedRequest {
    pub request: ScoreBySongRequest,
    pub timeout: Duration,
}

pub fn request(args: QueryScoreBySongArgs) -> Result<ConvertedRequest, ScoreBySongToolError> {
    let player = player(args.qq, args.username, args.target)?;
    let group_id = args
        .group_id
        .map(GroupId::new)
        .transpose()
        .map_err(|_| ScoreBySongToolError::invalid("groupId 格式不正确。"))?;
    let query = text(args.song_query, "songQuery")?
        .ok_or_else(|| ScoreBySongToolError::invalid("必须提供 songQuery。"))?;
    let limit = args.search_limit.unwrap_or(5);
    if !(1..=20).contains(&limit) {
        return Err(ScoreBySongToolError::invalid(
            "searchLimit 必须是 1 到 20 之间的整数。",
        ));
    }
    let timeout_ms = args.timeout_ms.unwrap_or(10_000);
    if !(1_000..=30_000).contains(&timeout_ms) {
        return Err(ScoreBySongToolError::invalid(
            "timeoutMs 必须是 1000 到 30000 之间的整数。",
        ));
    }
    Ok(ConvertedRequest {
        request: ScoreBySongRequest {
            player,
            group_id,
            song: SongLookupRequest {
                query,
                difficulty: args.difficulty.map(difficulty).transpose()?,
                generation: args.song_type.map(generation).transpose()?,
                limit,
            },
            include_raw: args.include_raw,
            now: OffsetDateTime::now_utc(),
        },
        timeout: Duration::from_millis(timeout_ms),
    })
}

fn player(
    qq: Option<String>,
    username: Option<String>,
    target: Option<String>,
) -> Result<PlayerLookupRequest, ScoreBySongToolError> {
    let qq = text(qq, "qq")?;
    let username = text(username, "username")?;
    let target = text(target, "target")?;
    if usize::from(qq.is_some()) + usize::from(username.is_some()) + usize::from(target.is_some())
        != 1
    {
        return Err(ScoreBySongToolError::invalid(
            "必须且只能提供 qq、username、target 其中一个。",
        ));
    }
    if let Some(qq) = qq {
        return QqId::new(qq)
            .map(PlayerLookupRequest::Qq)
            .map_err(|_| ScoreBySongToolError::invalid("qq 必须是数字字符串。"));
    }
    if let Some(username) = username {
        return PlayerUsername::new(username)
            .map(PlayerLookupRequest::Username)
            .map_err(|_| ScoreBySongToolError::invalid("username 格式不正确。"));
    }
    PlayerUsername::new(target.unwrap_or_default())
        .map(PlayerLookupRequest::Auto)
        .map_err(|_| ScoreBySongToolError::invalid("target 格式不正确。"))
}

fn text(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<String>, ScoreBySongToolError> {
    value
        .map(|value| {
            if value.chars().any(char::is_control) {
                return Err(ScoreBySongToolError::invalid(format!(
                    "{field} 格式不正确。"
                )));
            }
            let value = value.trim().to_owned();
            Ok((!value.is_empty()).then_some(value))
        })
        .transpose()
        .map(Option::flatten)
}

fn difficulty(value: String) -> Result<Difficulty, ScoreBySongToolError> {
    let value = value.trim().to_lowercase().replace([':', '：', ' '], "");
    match value.as_str() {
        "basic" | "bas" | "绿" | "绿色" => Ok(Difficulty::Basic),
        "advanced" | "adv" | "黄" | "黄色" => Ok(Difficulty::Advanced),
        "expert" | "exp" | "红" | "红色" => Ok(Difficulty::Expert),
        "master" | "mas" | "紫" | "紫色" => Ok(Difficulty::Master),
        "remaster" | "remas" | "re-master" | "白" | "白色" => Ok(Difficulty::ReMaster),
        "utage" | "宴" => Ok(Difficulty::Utage),
        _ => Err(ScoreBySongToolError::invalid("difficulty 格式不正确。")),
    }
}

fn generation(value: String) -> Result<SongGenerationFilter, ScoreBySongToolError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "standard" | "sd" => Ok(SongGenerationFilter::Exact(ChartGeneration::Standard)),
        "dx" => Ok(SongGenerationFilter::Exact(ChartGeneration::Deluxe)),
        "utage" | "宴" => Ok(SongGenerationFilter::UtageAny),
        _ => Err(ScoreBySongToolError::invalid("songType 格式不正确。")),
    }
}
