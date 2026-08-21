use maimai_app::{music_score::MusicScoreRequest, scores::Lookup};
use maimai_core::{PlayerUsername, QqId, ScoreSource};

use super::{dto::MusicScoreArgs, handler::MusicScoreSurface};
use crate::render_tools::{RenderToolError, dto::Scalar, score_source};

pub(super) fn request(
    args: MusicScoreArgs,
    surface: MusicScoreSurface,
) -> Result<MusicScoreRequest, RenderToolError> {
    let player = player(args.music.qq.as_ref(), args.music.username.as_ref())?;
    let raw_source = args
        .score_source_camel
        .as_deref()
        .or(args.score_source.as_deref())
        .or(args.data_source_camel.as_deref())
        .or(args.data_source.as_deref())
        .or(args.source.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if surface == MusicScoreSurface::Public && raw_source.is_some() {
        return Err(RenderToolError::invalid("public 版本不接受 source 参数"));
    }
    let source = if surface == MusicScoreSurface::Public {
        Some(ScoreSource::DivingFish)
    } else {
        raw_source.map(source).transpose()?
    };
    let mut music = super::super::convert::request(args.music, None)?;
    if music.music_id.is_some() {
        music.query = None;
    }
    Ok(MusicScoreRequest {
        music,
        player,
        source,
    })
}

fn player(qq: Option<&Scalar>, username: Option<&Scalar>) -> Result<Lookup, RenderToolError> {
    let qq = value(qq);
    let username = value(username);
    match (qq, username) {
        (Some(_), Some(_)) | (None, None) => Err(RenderToolError::invalid(
            "qq 和 username 必须且只能提供一个",
        )),
        (Some(qq), None) => QqId::new(qq)
            .map(Lookup::Qq)
            .map_err(|_| RenderToolError::invalid("qq 必须是数字字符串")),
        (None, Some(username)) => PlayerUsername::new(username)
            .map(Lookup::Username)
            .map_err(|_| RenderToolError::invalid("username 格式不正确")),
    }
}

fn value(value: Option<&Scalar>) -> Option<&str> {
    match value? {
        Scalar::Text(value) => Some(value.trim()).filter(|value| !value.is_empty()),
        Scalar::Number(_) => None,
    }
}

fn source(value: &str) -> Result<ScoreSource, RenderToolError> {
    score_source::parse(value)
        .ok_or_else(|| RenderToolError::invalid("source 必须是 local、sy 或 lxns"))
}
