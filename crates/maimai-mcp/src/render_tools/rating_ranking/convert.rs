use maimai_app::rating_ranking::{RatingRankingError, RatingRankingTarget, RatingRankingUsername};
use maimai_core::QqId;

use super::super::dto::Scalar;
use super::dto::RatingRankingArgs;
use crate::render_tools::RenderToolError;

pub(super) fn target(args: RatingRankingArgs) -> Result<RatingRankingTarget, RenderToolError> {
    if let Some(value) = first_nonempty([args.name.as_ref(), args.username.as_ref()]) {
        return RatingRankingUsername::new(value)
            .map(RatingRankingTarget::username)
            .map_err(app_error);
    }
    if let Some(value) = nonempty(args.qq.as_ref()) {
        return QqId::new(value)
            .map(RatingRankingTarget::qq)
            .map_err(|_| RenderToolError::invalid("qq 必须是非空数字字符串"));
    }
    if args.start_rank.is_some() || args.end_rank.is_some() {
        let start = parse_rank(args.start_rank.as_ref(), 1, "startRank")?;
        let end = parse_rank(args.end_rank.as_ref(), start, "endRank")?;
        return RatingRankingTarget::range(start, end).map_err(app_error);
    }
    let page = parse_rank(args.page.as_ref(), 1, "page")?;
    RatingRankingTarget::page(page).map_err(app_error)
}

pub(super) fn error_text(error: &RatingRankingError) -> String {
    match error {
        RatingRankingError::InvalidInput(_)
        | RatingRankingError::MissingUsername { .. }
        | RatingRankingError::InvalidProviderUsername => error.to_string(),
        RatingRankingError::Provider(_)
        | RatingRankingError::TaskJoin
        | RatingRankingError::Render(_)
        | RatingRankingError::Output(_) => {
            format!("渲染 rating 排行榜失败: {error}")
        }
    }
}

fn first_nonempty(values: [Option<&Scalar>; 2]) -> Option<String> {
    values.into_iter().find_map(nonempty)
}

fn nonempty(value: Option<&Scalar>) -> Option<String> {
    value
        .map(Scalar::text)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_rank(
    value: Option<&Scalar>,
    default: usize,
    field: &'static str,
) -> Result<usize, RenderToolError> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .text()
        .trim()
        .parse::<usize>()
        .map_err(|_| RenderToolError::invalid(format!("{field} 必须是正整数")))
}

fn app_error(error: RatingRankingError) -> RenderToolError {
    RenderToolError::invalid(error.to_string())
}
