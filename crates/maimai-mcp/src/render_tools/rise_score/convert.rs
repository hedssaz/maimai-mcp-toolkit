use maimai_app::{
    rise_score::{RiseScoreAlgorithm, RiseScoreRequest},
    scores::Lookup,
};
use maimai_core::{PlayerUsername, QqId, ScoreSource};
use time::OffsetDateTime;

use super::{RiseScoreSurface, dto::RiseScoreArgs, error::RiseScoreToolError};
use crate::render_tools::{dto::Scalar, score_source};

pub(super) fn request(
    args: RiseScoreArgs,
    surface: RiseScoreSurface,
    now: OffsetDateTime,
) -> Result<RiseScoreRequest, RiseScoreToolError> {
    let lookup = lookup(args.qq.as_ref(), args.username.as_ref())?;
    let level = optional_text(args.level.as_ref(), "level 必须是字符串")?;
    if level
        .as_ref()
        .is_some_and(|value| value.chars().any(char::is_control) || value.chars().count() > 16)
    {
        return Err(RiseScoreToolError::invalid("level 必须是有效等级字符串"));
    }
    let score = score(args.score.as_ref())?;
    let algorithm = algorithm(args.algorithm.as_ref())?;
    let source_value = first_nonempty([
        args.score_source_camel.as_ref(),
        args.score_source.as_ref(),
        args.data_source_camel.as_ref(),
        args.data_source.as_ref(),
        args.source.as_ref(),
    ]);
    let source = match surface {
        RiseScoreSurface::Main => source_value.as_ref().map(source).transpose()?,
        RiseScoreSurface::Public => {
            if source_value.is_some() {
                return Err(RiseScoreToolError::invalid(
                    "公开版 render_maimai_rise_score 不接受来源字段",
                ));
            }
            Some(ScoreSource::DivingFish)
        }
    };
    Ok(RiseScoreRequest {
        lookup,
        source,
        level,
        score,
        algorithm,
        now,
    })
}

fn lookup(qq: Option<&Scalar>, username: Option<&Scalar>) -> Result<Lookup, RiseScoreToolError> {
    let qq = nonempty(qq);
    let username = nonempty(username);
    match (qq, username) {
        (Some(_), Some(_)) => Err(RiseScoreToolError::invalid("qq 与 username 必须严格二选一")),
        (Some(value), None) => QqId::new(value)
            .map(Lookup::Qq)
            .map_err(|_| RiseScoreToolError::invalid("qq 必须是非空数字字符串")),
        (None, Some(value)) => PlayerUsername::new(value)
            .map(Lookup::Username)
            .map_err(|_| RiseScoreToolError::invalid("username 格式不正确")),
        (None, None) => Err(RiseScoreToolError::invalid(
            "需要提供 qq 或 username，且只能提供其中一个",
        )),
    }
}

fn optional_text(
    value: Option<&Scalar>,
    message: &str,
) -> Result<Option<String>, RiseScoreToolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Scalar::Text(value) => Ok((!value.trim().is_empty()).then(|| value.trim().to_owned())),
        Scalar::Number(_) => Err(RiseScoreToolError::invalid(message)),
    }
}

fn score(value: Option<&Scalar>) -> Result<Option<u32>, RiseScoreToolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Scalar::Number(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| RiseScoreToolError::invalid("score 必须是非负整数")),
        Scalar::Text(_) => Err(RiseScoreToolError::invalid("score 必须是非负整数")),
    }
}

fn algorithm(value: Option<&Scalar>) -> Result<RiseScoreAlgorithm, RiseScoreToolError> {
    let Some(value) = value else {
        return Ok(RiseScoreAlgorithm::Legacy);
    };
    let Scalar::Text(value) = value else {
        return Err(RiseScoreToolError::invalid(
            "algorithm 必须是 legacy 或 expected",
        ));
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "legacy" => Ok(RiseScoreAlgorithm::Legacy),
        "expected" => Ok(RiseScoreAlgorithm::Expected),
        _ => Err(RiseScoreToolError::invalid(
            "algorithm 必须是 legacy 或 expected",
        )),
    }
}

fn source(value: &Scalar) -> Result<ScoreSource, RiseScoreToolError> {
    score_source::parse(&value.text())
        .ok_or_else(|| RiseScoreToolError::invalid("source 必须是 local、sy 或 lxns"))
}

fn nonempty(value: Option<&Scalar>) -> Option<String> {
    value
        .map(Scalar::text)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn first_nonempty<const N: usize>(values: [Option<&Scalar>; N]) -> Option<Scalar> {
    values.into_iter().flatten().find_map(|value| {
        (!value.text().trim().is_empty()).then(|| match value {
            Scalar::Text(value) => Scalar::Text(value.clone()),
            Scalar::Number(value) => Scalar::Number(value.clone()),
        })
    })
}
