use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use maimai_app::catalog_refresh::{
    CatalogRefreshService, DEFAULT_TIMEOUT_SECONDS, DEFAULT_TTL_DAYS, EnabledSources,
    OperationOutcome, RefreshRequest, RefreshResult, ReloadSummary, SourceStatus,
    job::CatalogRefreshJobs, source_label, timeout_duration,
};
use maimai_providers::CatalogSource;
use serde_json::{Map, Number, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    dto::{Boolish, OneOrManyString, RefreshArgs, Scalar},
    error::CatalogToolError,
};

pub(super) async fn execute(
    service: &Arc<CatalogRefreshService>,
    jobs: Option<&Arc<CatalogRefreshJobs>>,
    args: &RefreshArgs,
) -> Result<Value, CatalogToolError> {
    let sources = parse_sources(args, service.enabled_sources())?;
    let ttl_days = parse_ttl(args.ttl_days.as_ref().or(args.source_ttl_days.as_ref()))?;
    let force = parse_bool(args.force.as_ref(), false)?;
    let check_only = parse_bool(args.check_only.as_ref(), false)?;
    let timeout = parse_timeout(args.timeout_seconds.as_ref())?;
    let request = RefreshRequest::new(sources, ttl_days, force, check_only, timeout)?;
    let background = background_requested(
        force,
        check_only,
        parse_bool(args.background.as_ref().or(args.bg.as_ref()), false)?,
    );
    if background {
        let jobs = jobs.ok_or_else(|| {
            CatalogToolError::input("catalog source refresh jobs are not configured")
        })?;
        let started = jobs.start(request).await?;
        return super::refresh_job::start_value(&started);
    }
    let result = service.refresh(request).await?;
    result_value(&result)
}

fn parse_sources(
    args: &RefreshArgs,
    enabled: &EnabledSources,
) -> Result<Vec<CatalogSource>, CatalogToolError> {
    let raw = if let Some(sources) = &args.sources {
        match sources {
            OneOrManyString::One(value) => vec![value.clone()],
            OneOrManyString::Many(values) => values.clone(),
        }
    } else if let Some(source) = &args.source {
        vec![source.clone()]
    } else {
        vec!["all".to_owned()]
    };
    let mut normalized = Vec::new();
    for raw_value in raw {
        let values = if raw_value.trim().is_empty() {
            vec!["all"]
        } else {
            raw_value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect()
        };
        for value in values {
            let key = value.to_ascii_lowercase().replace(['-', ' '], "_");
            if key == "all" {
                return Ok(enabled.sources().to_vec());
            }
            let source = source_alias(&key).ok_or_else(|| {
                CatalogToolError::input(format!("INVALID_SOURCE: unknown source: {value}"))
            })?;
            if !enabled.contains(source) {
                return Err(CatalogToolError::input(format!(
                    "INVALID_SOURCE: source {} is not enabled on this server",
                    source.name()
                )));
            }
            if !normalized.contains(&source) {
                normalized.push(source);
            }
        }
    }
    if normalized.is_empty() {
        return Err(CatalogToolError::input("at least one source is required"));
    }
    Ok(normalized)
}

pub(super) fn source_alias(value: &str) -> Option<CatalogSource> {
    match value {
        "lxns" | "落雪" | "cn" => Some(CatalogSource::Lxns),
        "dxdata" | "dxrating" | "jp" | "日版" | "日服" | "maidb" => Some(CatalogSource::DxData),
        "divingfish" | "水鱼" | "df" => Some(CatalogSource::DivingFish),
        "yuzu" | "柚子" | "yuzu_alias" | "yuzu_aliases" | "music_alias" | "别名补充" => {
            Some(CatalogSource::Yuzu)
        }
        "chart_stats" | "chartstats" | "fit" | "拟合定数" => Some(CatalogSource::ChartStats),
        "dxrating_aliases" | "dxrating_alias" | "社区别名" => {
            Some(CatalogSource::DxRatingAliases)
        }
        "dxrating_tags" | "dxrating_tag" | "社区标签" => Some(CatalogSource::DxRatingTags),
        "plate" | "牌子" | "plate_data" => Some(CatalogSource::Plate),
        "location" | "locations" | "maidx_location" | "maidx_locations" | "shop" | "shops"
        | "机厅" | "地址" => Some(CatalogSource::Location),
        _ => None,
    }
}

fn parse_ttl(value: Option<&Scalar>) -> Result<f64, CatalogToolError> {
    let Some(value) = value else {
        return Ok(DEFAULT_TTL_DAYS);
    };
    let text = value.text();
    if text.is_empty() {
        return Ok(DEFAULT_TTL_DAYS);
    }
    let days = text
        .parse::<f64>()
        .map_err(|_| CatalogToolError::input("ttl_days must be a number"))?;
    if !days.is_finite() || days < 0.0 {
        return Err(CatalogToolError::input("ttl_days must be >= 0"));
    }
    Ok(days)
}

fn parse_timeout(value: Option<&Scalar>) -> Result<Duration, CatalogToolError> {
    let seconds = match value {
        None => DEFAULT_TIMEOUT_SECONDS,
        Some(Scalar::Text(value)) => value
            .trim()
            .parse::<u64>()
            .map_err(|_| CatalogToolError::input("timeout_seconds must be an integer"))?,
        Some(Scalar::Number(value)) => integer_number(value)?,
    };
    timeout_duration(seconds).map_err(CatalogToolError::from)
}

fn integer_number(value: &Number) -> Result<u64, CatalogToolError> {
    if let Some(value) = value.as_u64() {
        return Ok(value);
    }
    let value = value
        .as_f64()
        .filter(|value| value.is_finite() && value.fract() == 0.0 && *value >= 0.0)
        .ok_or_else(|| CatalogToolError::input("timeout_seconds must be an integer"))?;
    if value > u64::MAX as f64 {
        return Err(CatalogToolError::input(
            "timeout_seconds is outside the supported range",
        ));
    }
    Ok(value as u64)
}

fn parse_bool(value: Option<&Boolish>, default: bool) -> Result<bool, CatalogToolError> {
    let Some(value) = value else {
        return Ok(default);
    };
    let text = match value {
        Boolish::Bool(value) => return Ok(*value),
        Boolish::Scalar(value) => value.text().to_ascii_lowercase(),
    };
    match text.as_str() {
        "1" | "true" | "yes" | "y" | "on" => Ok(true),
        "0" | "false" | "no" | "n" | "off" => Ok(false),
        _ => Err(CatalogToolError::input(
            "boolean value must be true or false",
        )),
    }
}

fn result_value(result: &RefreshResult) -> Result<Value, CatalogToolError> {
    let mut sources = Map::new();
    for status in result.statuses() {
        sources.insert(status.source().name().to_owned(), status_value(status)?);
    }
    let mut commands = result
        .operations()
        .iter()
        .map(operation_value)
        .collect::<Vec<_>>();
    if let ReloadSummary::Failed { message } = result.reload() {
        commands.push(json!({
            "source": "catalog_reload",
            "operation": "reload_catalog_snapshot",
            "outcome": "failed",
            "returncode": 1,
            "error_code": "CATALOG_RELOAD_FAILED",
            "error": message,
            "disk_updated": true
        }));
    }
    Ok(json!({
        "ttl_days": result.ttl_days(),
        "force": result.force(),
        "check_only": result.check_only(),
        "requested_sources": names(result.requested_sources()),
        "due_sources": names(result.due_sources()),
        "refreshed_sources": names(result.refreshed_sources()),
        "skipped_sources": names(result.skipped_sources()),
        "failed_sources": names(result.failed_sources()),
        "sources": sources,
        "commands": commands,
        "derived_updated": matches!(result.reload(), ReloadSummary::Reloaded),
        "catalog_reload": match result.reload() {
            ReloadSummary::NotNeeded => "not_needed",
            ReloadSummary::Reloaded => "reloaded",
            ReloadSummary::Failed { .. } => "failed",
        }
    }))
}

fn status_value(status: &SourceStatus) -> Result<Value, CatalogToolError> {
    let targets = status
        .targets()
        .iter()
        .map(|target| {
            Ok(json!({
                "target": format!("data/{}", target.target().file_name()),
                "exists": target.exists(),
                "mtime": timestamp(target.modified())?,
                "age_seconds": target.age().map(|age| round(age.as_secs_f64(), 3)),
                "age_days": target.age().map(|age| round(age.as_secs_f64() / 86_400.0, 6)),
                "expired": target.expired()
            }))
        })
        .collect::<Result<Vec<_>, CatalogToolError>>()?;
    let missing = status
        .targets()
        .iter()
        .filter(|target| !target.exists())
        .map(|target| format!("data/{}", target.target().file_name()))
        .collect::<Vec<_>>();
    let expired = status
        .targets()
        .iter()
        .filter(|target| target.expired())
        .map(|target| format!("data/{}", target.target().file_name()))
        .collect::<Vec<_>>();
    Ok(json!({
        "source": status.source().name(),
        "label": source_label(status.source()),
        "targets": status.targets().iter().map(|target| format!("data/{}", target.target().file_name())).collect::<Vec<_>>(),
        "exists": status.exists(),
        "missing_targets": missing,
        "expired_targets": expired,
        "target_statuses": targets,
        "mtime": timestamp(status.oldest_modified())?,
        "age_seconds": status.age().map(|age| round(age.as_secs_f64(), 3)),
        "age_days": status.age().map(|age| round(age.as_secs_f64() / 86_400.0, 6)),
        "ttl_days": status.ttl_days(),
        "expired": status.expired()
    }))
}

fn operation_value(operation: &maimai_app::catalog_refresh::OperationSummary) -> Value {
    let outcome = match operation.outcome() {
        OperationOutcome::Updated => "updated",
        OperationOutcome::NotModified => "not_modified",
        OperationOutcome::Failed => "failed",
        OperationOutcome::DiskUpdatedPendingReload => "disk_updated_pending_reload",
    };
    json!({
        "source": operation.source().name(),
        "operation": "download_validate_publish",
        "outcome": outcome,
        "returncode": if operation.error().is_some() { 1 } else { 0 },
        "timeout": operation.error_code() == Some("TIMEOUT"),
        "duration_seconds": round(operation.duration().as_secs_f64(), 3),
        "error_code": operation.error_code(),
        "error": operation.error(),
        "disk_updated": operation.disk_updated()
    })
}

fn timestamp(value: Option<SystemTime>) -> Result<Option<String>, CatalogToolError> {
    value
        .map(|value| OffsetDateTime::from(value).format(&Rfc3339))
        .transpose()
        .map_err(|_| CatalogToolError::input("source timestamp is outside the supported range"))
}

fn names(sources: &[CatalogSource]) -> Vec<&'static str> {
    sources.iter().map(|source| source.name()).collect()
}

pub(super) fn round(value: f64, decimals: i32) -> f64 {
    let scale = 10_f64.powi(decimals);
    (value * scale).round() / scale
}

const fn background_requested(force: bool, check_only: bool, explicit: bool) -> bool {
    !check_only && (force || explicit)
}

#[cfg(test)]
mod tests {
    use super::background_requested;

    #[test]
    fn force_alone_selects_background_but_check_only_stays_synchronous() {
        assert!(background_requested(true, false, false));
        assert!(background_requested(false, false, true));
        assert!(!background_requested(true, true, true));
    }
}
