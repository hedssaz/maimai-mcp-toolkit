use std::sync::Arc;

use maimai_app::catalog_refresh::{
    SourceStatus,
    job::{
        CatalogRefreshJob, CatalogRefreshJobId, CatalogRefreshJobOutcome, CatalogRefreshJobStatus,
        CatalogRefreshJobs, CatalogRefreshSourceStatus, StartRefreshJob,
    },
    source_label,
};
use serde_json::{Map, Value, json};
use time::format_description::well_known::Rfc3339;

use super::{
    dto::RefreshJobStatusArgs,
    error::CatalogToolError,
    refresh::{round, source_alias},
};

pub(super) async fn job_status(
    jobs: &Arc<CatalogRefreshJobs>,
    args: &RefreshJobStatusArgs,
) -> Result<Value, CatalogToolError> {
    let id = CatalogRefreshJobId::parse(args.job_id.trim())
        .map_err(|_| CatalogToolError::input("INVALID_JOB_ID: refresh job id is invalid"))?;
    let Some(job) = jobs.status(id).await? else {
        return Ok(json!({"error": format!("未找到刷新任务 {}", args.job_id.trim())}));
    };
    job_value(&job)
}

pub(super) fn start_value(started: &StartRefreshJob) -> Result<Value, CatalogToolError> {
    let job = started.record();
    let mut source_states = Map::new();
    for source in &job.sources {
        let status = started.plan().and_then(|plan| {
            plan.statuses()
                .iter()
                .find(|status| status.source().name() == source.source)
        });
        source_states.insert(
            source.source.clone(),
            json!({
                "expired": status.map_or(source.due, SourceStatus::expired),
                "age_days": status.and_then(SourceStatus::age).map(|age| round(age.as_secs_f64() / 86_400.0, 6)),
                "label": source_name_label(&source.source)
            }),
        );
    }
    Ok(json!({
        "background": true,
        "started": started.started(),
        "jobId": job.id.to_string(),
        "status": outward_status(job.status),
        "totalSources": job.sources.len(),
        "dueSources": job.sources.iter().filter(|source| source.due).count(),
        "sourceStates": source_states,
        "message": job.message
    }))
}

fn job_value(job: &CatalogRefreshJob) -> Result<Value, CatalogToolError> {
    let mut sources = Map::new();
    for source in &job.sources {
        sources.insert(
            source.source.clone(),
            json!({
                "source": source.source,
                "operation": "download_validate_publish",
                "outcome": source.status.as_str(),
                "returncode": if matches!(
                    source.status,
                    CatalogRefreshSourceStatus::Failed
                        | CatalogRefreshSourceStatus::DiskUpdatedPendingReload
                ) { 1 } else { 0 },
                "duration_seconds": source.duration_millis.map(|value| round(value as f64 / 1000.0, 3)),
                "error_code": source.error_code,
                "error": source.error_message
            }),
        );
    }
    Ok(json!({
        "jobId": job.id.to_string(),
        "status": outward_status(job.status),
        "outcome": job.outcome.map(CatalogRefreshJobOutcome::as_str),
        "createdAt": job.created_at.format(&Rfc3339)?,
        "startedAt": job.started_at.map(|value| value.format(&Rfc3339)).transpose()?,
        "finishedAt": job.finished_at.map(|value| value.format(&Rfc3339)).transpose()?,
        "totalSources": job.sources.len(),
        "dueSources": job.sources.iter().filter(|source| source.due).map(|source| source.source.as_str()).collect::<Vec<_>>(),
        "completedSources": job.completed_sources(),
        "succeededSources": job.succeeded_sources(),
        "failedSources": job.failed_sources(),
        "skippedSources": job.skipped_sources(),
        "sources": sources,
        "message": job.message,
        "errorCode": job.error_code,
        "error": job.error_message
    }))
}

fn source_name_label(source: &str) -> &'static str {
    source_alias(source)
        .map(source_label)
        .unwrap_or("未知数据源")
}

fn outward_status(status: CatalogRefreshJobStatus) -> &'static str {
    match status {
        CatalogRefreshJobStatus::Completed => "finished",
        CatalogRefreshJobStatus::Queued => "queued",
        CatalogRefreshJobStatus::Running => "running",
        CatalogRefreshJobStatus::Failed => "failed",
        CatalogRefreshJobStatus::Interrupted => "interrupted",
    }
}

#[cfg(test)]
mod tests {
    use maimai_app::catalog_refresh::job::{
        CatalogRefreshJob, CatalogRefreshJobId, CatalogRefreshJobOutcome, CatalogRefreshJobSource,
        CatalogRefreshJobStatus, CatalogRefreshSourceStatus,
    };
    use time::OffsetDateTime;

    use super::job_value;

    #[test]
    fn partial_failure_keeps_finished_status_with_explicit_outcome()
    -> Result<(), Box<dyn std::error::Error>> {
        let now = OffsetDateTime::now_utc();
        let value = job_value(&CatalogRefreshJob {
            id: CatalogRefreshJobId::parse("1")?,
            status: CatalogRefreshJobStatus::Completed,
            outcome: Some(CatalogRefreshJobOutcome::PartialFailure),
            created_at: now,
            started_at: Some(now),
            finished_at: Some(now),
            message: "刷新完成: 成功 1/2，失败 1".to_owned(),
            error_code: Some("SOURCE_FAILURE".to_owned()),
            error_message: Some("one or more catalog sources were not published".to_owned()),
            sources: vec![
                source("lxns", CatalogRefreshSourceStatus::Updated, None),
                source(
                    "dxdata",
                    CatalogRefreshSourceStatus::Failed,
                    Some("NETWORK"),
                ),
            ],
        })?;
        assert_eq!(value["status"], "finished");
        assert_eq!(value["outcome"], "partial_failure");
        assert_eq!(value["succeededSources"], serde_json::json!(["lxns"]));
        assert_eq!(value["failedSources"], serde_json::json!(["dxdata"]));
        Ok(())
    }

    fn source(
        name: &str,
        status: CatalogRefreshSourceStatus,
        error: Option<&str>,
    ) -> CatalogRefreshJobSource {
        CatalogRefreshJobSource {
            position: u32::from(name == "dxdata"),
            source: name.to_owned(),
            due: true,
            status,
            duration_millis: Some(10),
            error_code: error.map(str::to_owned),
            error_message: error.map(|_| "source request failed".to_owned()),
        }
    }
}
