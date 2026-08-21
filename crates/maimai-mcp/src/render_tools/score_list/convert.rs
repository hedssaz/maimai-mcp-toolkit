use maimai_app::{
    score_list::{ScoreListRequest, ScoreListTarget},
    scores::Lookup,
};
use maimai_core::{ChartConstant, PlayerUsername, QqId, ScoreSource};
use rust_decimal::Decimal;
use time::OffsetDateTime;

use super::{ScoreListSurface, dto::ScoreListArgs, error::ScoreListToolError};
use crate::render_tools::{dto::Scalar, score_source};

pub(super) fn request(
    args: ScoreListArgs,
    surface: ScoreListSurface,
    now: OffsetDateTime,
) -> Result<ScoreListRequest, ScoreListToolError> {
    let lookup = lookup(args.qq.as_ref(), args.username.as_ref())?;
    let target = target(args.ds.as_ref(), args.rating.as_ref(), args.level.as_ref())?;
    let page = page(args.page.as_ref())?;
    let source_value = first_nonempty([
        args.score_source_camel.as_ref(),
        args.score_source.as_ref(),
        args.data_source_camel.as_ref(),
        args.data_source.as_ref(),
        args.source.as_ref(),
    ]);
    let source = match surface {
        ScoreListSurface::Main => source_value.as_ref().map(source).transpose()?,
        ScoreListSurface::Public => {
            if source_value.is_some() {
                return Err(ScoreListToolError::invalid(
                    "公开版 render_maimai_score_list 不接受 source/scoreSource/score_source",
                ));
            }
            Some(ScoreSource::DivingFish)
        }
    };
    Ok(ScoreListRequest {
        lookup,
        source,
        target,
        page,
        now,
    })
}

fn lookup(qq: Option<&Scalar>, username: Option<&Scalar>) -> Result<Lookup, ScoreListToolError> {
    let qq = nonempty(qq);
    let username = nonempty(username);
    match (qq, username) {
        (Some(_), Some(_)) => Err(ScoreListToolError::invalid("qq 与 username 必须严格二选一")),
        (Some(value), None) => QqId::new(value)
            .map(Lookup::Qq)
            .map_err(|_| ScoreListToolError::invalid("qq 必须是非空数字字符串")),
        (None, Some(value)) => PlayerUsername::new(value)
            .map(Lookup::Username)
            .map_err(|_| ScoreListToolError::invalid("username 格式不正确")),
        (None, None) => Err(ScoreListToolError::invalid(
            "需要提供 qq 或 username，且只能提供其中一个",
        )),
    }
}

fn target(
    ds: Option<&Scalar>,
    rating: Option<&Scalar>,
    level: Option<&Scalar>,
) -> Result<ScoreListTarget, ScoreListToolError> {
    if let Some(value) = ds {
        return match value {
            Scalar::Number(value) => constant(&value.to_string()),
            Scalar::Text(_) => Err(ScoreListToolError::invalid("ds 必须是数字")),
        };
    }
    if let Some(value) = rating.filter(|value| !value.text().trim().is_empty()) {
        return match value {
            Scalar::Text(value) if value.trim().contains('.') => constant(value),
            Scalar::Text(value) => ScoreListTarget::level(value).map_err(app_input),
            Scalar::Number(value) => numeric_rating(&value.to_string()),
        };
    }
    if let Some(value) = level.filter(|value| !value.text().trim().is_empty()) {
        return match value {
            Scalar::Text(value) => ScoreListTarget::level(value).map_err(app_input),
            Scalar::Number(_) => Err(ScoreListToolError::invalid("level 必须是字符串")),
        };
    }
    Err(ScoreListToolError::invalid("需要提供 rating/level/ds"))
}

fn numeric_rating(value: &str) -> Result<ScoreListTarget, ScoreListToolError> {
    let decimal = value
        .parse::<Decimal>()
        .map_err(|_| ScoreListToolError::invalid("rating 必须是有效的非负数字"))?;
    if decimal.fract().is_zero() {
        ScoreListTarget::level(decimal.trunc().to_string()).map_err(app_input)
    } else {
        constant(value)
    }
}

fn constant(value: &str) -> Result<ScoreListTarget, ScoreListToolError> {
    ChartConstant::from_decimal_str(value.trim())
        .map(ScoreListTarget::constant)
        .map_err(|_| ScoreListToolError::invalid("ds/rating 必须是有效的非负定数"))
}

fn page(value: Option<&Scalar>) -> Result<usize, ScoreListToolError> {
    let Some(value) = value else {
        return Ok(1);
    };
    value
        .text()
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| ScoreListToolError::invalid("page 必须是正整数"))
}

fn source(value: &Scalar) -> Result<ScoreSource, ScoreListToolError> {
    score_source::parse(&value.text())
        .ok_or_else(|| ScoreListToolError::invalid("source 必须是 local、sy 或 lxns"))
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

fn app_input(error: maimai_app::score_list::ScoreListError) -> ScoreListToolError {
    ScoreListToolError::invalid(error.to_string())
}
