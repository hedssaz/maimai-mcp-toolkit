mod alias_tools;
mod convert;
mod dto;
mod error;
mod format;
mod list_history;
mod random_today;
mod refresh;
mod refresh_job;
mod scoring_tools;
mod search_ops;
mod serialize;

use std::{future::Future, sync::Arc};

use maimai_app::catalog_refresh::{CatalogRefreshService, job::CatalogRefreshJobs};
use maimai_catalog::{CatalogSnapshot, CatalogStore};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{DispatchError, ToolCall, ToolDispatcher, ToolFailure, ToolOutput};
use dto::{
    AliasListArgs, AliasMutationArgs, BatchArgs, HistoryArgs, ListByIdArgs, RandomArgs,
    RefreshArgs, RefreshJobStatusArgs, SearchArgs, TodayArgs, VersionsArgs,
};
use error::CatalogToolError;
use format::{FormatOptions, render};

#[cfg(test)]
const TOOLS: [&str; 14] = [
    "search_maimai_songs",
    "batch_search_maimai_songs",
    "add_maimai_alias",
    "delete_maimai_alias",
    "list_maimai_aliases",
    "refresh_maimai_sources",
    "refresh_maimai_sources_job_status",
    "random_maimai_songs",
    "today_maimai",
    "list_maimai_songs_by_id",
    "list_maimai_versions",
    "query_chart_history",
    "score_counts",
    "find_score_combinations",
];

#[derive(Clone)]
pub struct CatalogDispatcher {
    store: Arc<CatalogStore>,
    refresh: Arc<CatalogRefreshService>,
    refresh_jobs: Arc<CatalogRefreshJobs>,
}

impl CatalogDispatcher {
    pub fn new(
        store: Arc<CatalogStore>,
        refresh: Arc<CatalogRefreshService>,
        refresh_jobs: Arc<CatalogRefreshJobs>,
    ) -> Self {
        Self {
            store,
            refresh,
            refresh_jobs,
        }
    }
}

impl ToolDispatcher for CatalogDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let store = Arc::clone(&self.store);
        let refresh = Arc::clone(&self.refresh);
        let refresh_jobs = Arc::clone(&self.refresh_jobs);
        async move { dispatch_call(&store, Some(&refresh), Some(&refresh_jobs), call).await }
    }
}

async fn dispatch_call(
    store: &CatalogStore,
    refresh_service: Option<&Arc<CatalogRefreshService>>,
    refresh_jobs: Option<&Arc<CatalogRefreshJobs>>,
    call: ToolCall,
) -> Result<ToolOutput, DispatchError> {
    let name = call.name().to_owned();
    let arguments = call.into_arguments();
    let result = execute_call(store, refresh_service, refresh_jobs, &name, arguments).await;
    match result {
        Ok((value, options)) => render(&name, &value, options)
            .map(ToolOutput::text)
            .map_err(|error| ToolFailure::text(error.to_string()).into()),
        Err(error) => Err(ToolFailure::text(error.to_string()).into()),
    }
}

async fn execute_call(
    store: &CatalogStore,
    refresh_service: Option<&Arc<CatalogRefreshService>>,
    refresh_jobs: Option<&Arc<CatalogRefreshJobs>>,
    name: &str,
    arguments: Map<String, Value>,
) -> Result<(Value, FormatOptions<'static>), CatalogToolError> {
    match name {
        "refresh_maimai_sources" => {
            let args: RefreshArgs = deserialize(arguments, name)?;
            let options = static_format(&args.format, args.include_raw, args.debug)?;
            let service = refresh_service.ok_or_else(|| {
                CatalogToolError::input("catalog source refresh service is not configured")
            })?;
            let value = refresh::execute(service, refresh_jobs, &args).await?;
            Ok((value, options))
        }
        "refresh_maimai_sources_job_status" => {
            let args: RefreshJobStatusArgs = deserialize(arguments, name)?;
            let options = static_format(&args.format, args.include_raw, args.debug)?;
            let jobs = refresh_jobs.ok_or_else(|| {
                CatalogToolError::input("catalog source refresh jobs are not configured")
            })?;
            let value = refresh_job::job_status(jobs, &args).await?;
            Ok((value, options))
        }
        "add_maimai_alias" => {
            let args: AliasMutationArgs = deserialize(arguments, name)?;
            let options = static_format(&args.format, args.include_raw, args.debug)?;
            let value = alias_tools::add(store, &args).await?;
            Ok((value, options))
        }
        "delete_maimai_alias" => {
            let args: AliasMutationArgs = deserialize(arguments, name)?;
            let options = static_format(&args.format, args.include_raw, args.debug)?;
            let value = alias_tools::delete(store, &args).await?;
            Ok((value, options))
        }
        "list_maimai_aliases" => {
            let args: AliasListArgs = deserialize(arguments, name)?;
            let options = static_format(&args.format, args.include_raw, args.debug)?;
            let value = alias_tools::list(store, &args).await?;
            Ok((value, options))
        }
        _ => {
            let snapshot = store.snapshot();
            execute(&snapshot, name, arguments)
        }
    }
}

fn execute(
    snapshot: &CatalogSnapshot,
    name: &str,
    arguments: Map<String, Value>,
) -> Result<(Value, FormatOptions<'static>), CatalogToolError> {
    // FormatOptions borrows no DTO here; owned defaults are collapsed into static
    // choices after execution to keep the dispatcher result self-contained.
    match name {
        "score_counts" | "find_score_combinations" => {
            scoring_tools::execute(snapshot, name, arguments)
        }
        "search_maimai_songs" => {
            let args: SearchArgs = deserialize(arguments, name)?;
            let value = search_ops::search(snapshot, &args)?;
            Ok((
                value,
                static_format(&args.format, args.include_raw, args.debug)?,
            ))
        }
        "batch_search_maimai_songs" => {
            let args: BatchArgs = deserialize(arguments, name)?;
            let value = search_ops::batch(snapshot, &args)?;
            Ok((
                value,
                static_format(&args.format, args.include_raw, args.debug)?,
            ))
        }
        "random_maimai_songs" => {
            let args: RandomArgs = deserialize(arguments, name)?;
            let value = random_today::random(snapshot, &args)?;
            Ok((
                value,
                static_format(
                    &args.search.format,
                    args.search.include_raw,
                    args.search.debug,
                )?,
            ))
        }
        "today_maimai" => {
            let args: TodayArgs = deserialize(arguments, name)?;
            let value = random_today::today(snapshot, &args)?;
            Ok((
                value,
                static_format(&args.format, args.include_raw, args.debug)?,
            ))
        }
        "list_maimai_songs_by_id" => {
            let args: ListByIdArgs = deserialize(arguments, name)?;
            let value = list_history::list_by_id(snapshot, &args)?;
            Ok((
                value,
                static_format(
                    &args.search.format,
                    args.search.include_raw,
                    args.search.debug,
                )?,
            ))
        }
        "list_maimai_versions" => {
            let args: VersionsArgs = deserialize(arguments, name)?;
            let value = list_history::versions(snapshot, &args)?;
            Ok((
                value,
                static_format(&args.format, args.include_raw, args.debug)?,
            ))
        }
        "query_chart_history" => {
            let args: HistoryArgs = deserialize(arguments, name)?;
            let value = list_history::history(snapshot, &args)?;
            Ok((
                value,
                static_format(&args.format, args.include_raw, args.debug)?,
            ))
        }
        _ => Err(CatalogToolError::input(format!(
            "unknown catalog tool: {name}"
        ))),
    }
}

fn static_format(
    format: &Option<String>,
    include_raw: Option<bool>,
    debug: Option<bool>,
) -> Result<FormatOptions<'static>, CatalogToolError> {
    let format = match format
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase())
    {
        None => None,
        Some(value) if value == "text" => Some("text"),
        Some(value) if value == "compact" => Some("compact"),
        Some(value) if value == "json" => Some("json"),
        Some(_) => {
            return Err(CatalogToolError::input(
                "format must be text, compact, or json",
            ));
        }
    };
    Ok(FormatOptions {
        format,
        include_raw: include_raw.is_some_and(|value| value),
        debug: debug.is_some_and(|value| value),
    })
}

fn deserialize<T: DeserializeOwned>(
    arguments: Map<String, Value>,
    tool: &str,
) -> Result<T, CatalogToolError> {
    serde_json::from_value(Value::Object(arguments)).map_err(|error| {
        CatalogToolError::input(format!("Invalid arguments for tool {tool}: {error}"))
    })
}

#[cfg(test)]
mod alias_tests;
#[cfg(test)]
mod tests;
