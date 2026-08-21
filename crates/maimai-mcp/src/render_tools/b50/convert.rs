use std::{path::PathBuf, time::Duration};

use maimai_app::{
    b50_image::B50ImageStyle,
    b50_render::B50RenderRequest,
    score_service::B50Mode,
    scores::{Lookup, RatingMode},
};
use maimai_core::{PlayerUsername, QqId, ScoreSource};
use time::OffsetDateTime;

use super::{dto::RenderB50Args, error::B50RenderToolError, handler::RenderDeployment};
use crate::render_tools::score_source;

pub(super) struct PreparedRequest {
    pub request: B50RenderRequest,
    pub timeout: Duration,
}

pub(super) fn request(
    args: RenderB50Args,
    deployment: RenderDeployment,
    now: OffsetDateTime,
) -> Result<PreparedRequest, B50RenderToolError> {
    let lookup = lookup(args.qq, args.username)?;
    let source_text = args
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let computed = args.compute_from_records || source_text.is_some_and(is_computed_alias);
    let source = if deployment == RenderDeployment::Public {
        Some(ScoreSource::DivingFish)
    } else {
        source_text
            .filter(|value| !is_computed_alias(value))
            .map(score_source)
            .transpose()?
    };
    let mut request = B50RenderRequest::new(lookup, now);
    request.source = source;
    request.mode = if computed {
        B50Mode::Computed(RatingMode::Fit)
    } else {
        B50Mode::Provider
    };
    request.style = style(args.style)?;
    request.title = optional_text(args.title, "title")?;
    request.static_dir = optional_path(args.static_dir, "staticDir")?;
    request.cover_cache_dir = optional_path(args.cover_cache_dir, "coverCacheDir")?;
    Ok(PreparedRequest {
        request,
        timeout: timeout(args.timeout_ms)?,
    })
}

fn lookup(qq: Option<String>, username: Option<String>) -> Result<Lookup, B50RenderToolError> {
    let qq = optional_text(qq, "qq")?;
    if let Some(qq) = qq {
        return QqId::new(qq)
            .map(Lookup::Qq)
            .map_err(|_| B50RenderToolError::invalid("qq 必须是数字字符串。"));
    }
    match optional_text(username, "username")? {
        None => Err(B50RenderToolError::invalid("需要提供 qq 或 username")),
        Some(username) => PlayerUsername::new(username)
            .map(Lookup::Username)
            .map_err(|_| B50RenderToolError::invalid("username 格式不正确。")),
    }
}

fn style(value: Option<String>) -> Result<B50ImageStyle, B50RenderToolError> {
    match value.as_deref() {
        None => Ok(B50ImageStyle::Yuzu),
        Some("yuzu") => Ok(B50ImageStyle::Yuzu),
        Some("maibot") => Ok(B50ImageStyle::Maibot),
        Some("legacy") => Ok(B50ImageStyle::Legacy),
        Some(value) => Err(B50RenderToolError::invalid(format!(
            "未知风格: {value}，可选 yuzu/maibot/legacy"
        ))),
    }
}

fn timeout(value: Option<u64>) -> Result<Duration, B50RenderToolError> {
    let value = value.unwrap_or(30_000);
    if !(1_000..=30_000).contains(&value) {
        return Err(B50RenderToolError::invalid(
            "timeoutMs 必须是 1000 到 30000 之间的整数。",
        ));
    }
    Ok(Duration::from_millis(value))
}

fn optional_text(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<String>, B50RenderToolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.chars().any(char::is_control) {
        return Err(B50RenderToolError::invalid(format!(
            "{field} 不能包含控制字符。"
        )));
    }
    let value = value.trim();
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

fn optional_path(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<PathBuf>, B50RenderToolError> {
    optional_text(value, field).map(|value| value.map(PathBuf::from))
}

fn is_computed_alias(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "records" | "computed" | "computed_b50" | "fit" | "fitted"
    )
}

fn score_source(value: &str) -> Result<ScoreSource, B50RenderToolError> {
    score_source::parse(value).ok_or_else(|| {
        B50RenderToolError::invalid("source 必须是 local、sy、lxns，或 records/computed/fitted。")
    })
}
