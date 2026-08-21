use maimai_app::music_global_stats::{
    MusicGlobalStatsDifficulty, MusicGlobalStatsError, MusicGlobalStatsRequest,
};

use super::dto::MusicGlobalStatsArgs;
use crate::render_tools::{RenderToolError, dto::Scalar};

pub(super) fn request(
    args: MusicGlobalStatsArgs,
) -> Result<MusicGlobalStatsRequest, RenderToolError> {
    let difficulty = difficulty(&args)?;
    let music = super::super::convert::request(args.music, None)?;
    Ok(MusicGlobalStatsRequest { music, difficulty })
}

fn difficulty(args: &MusicGlobalStatsArgs) -> Result<MusicGlobalStatsDifficulty, RenderToolError> {
    if let Some(index) = args
        .level_index
        .as_ref()
        .or(args.level_index_camel.as_ref())
        .or(args.difficulty_index.as_ref())
    {
        let index = parse_index(index)?;
        return MusicGlobalStatsDifficulty::from_index(index)
            .ok_or_else(|| RenderToolError::invalid("level_index/difficulty_index 必须是 0-4"));
    }
    let value = args
        .difficulty
        .as_deref()
        .or(args.diff.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("master")
        .to_lowercase();
    match value.as_str() {
        "basic" | "bas" | "green" | "绿" => Ok(MusicGlobalStatsDifficulty::Basic),
        "advanced" | "adv" | "yellow" | "黄" => Ok(MusicGlobalStatsDifficulty::Advanced),
        "expert" | "exp" | "red" | "红" => Ok(MusicGlobalStatsDifficulty::Expert),
        "master" | "mas" | "purple" | "紫" => Ok(MusicGlobalStatsDifficulty::Master),
        "remaster" | "re:master" | "remas" | "white" | "白" => {
            Ok(MusicGlobalStatsDifficulty::ReMaster)
        }
        _ => Err(RenderToolError::invalid(
            "difficulty 必须是 Basic/Advanced/Expert/Master/Re:MASTER 或绿/黄/红/紫/白",
        )),
    }
}

fn parse_index(value: &Scalar) -> Result<usize, RenderToolError> {
    value
        .text()
        .trim()
        .parse::<usize>()
        .map_err(|_| RenderToolError::invalid("level_index/difficulty_index 必须是 0-4"))
}

pub(super) fn error_text(error: &MusicGlobalStatsError) -> String {
    match error {
        MusicGlobalStatsError::InvalidInput(_) | MusicGlobalStatsError::MusicResolution(_) => {
            error.to_string()
        }
        MusicGlobalStatsError::TaskJoin
        | MusicGlobalStatsError::Render(_)
        | MusicGlobalStatsError::Output(_) => {
            format!("渲染全服统计图失败: {error}")
        }
    }
}
