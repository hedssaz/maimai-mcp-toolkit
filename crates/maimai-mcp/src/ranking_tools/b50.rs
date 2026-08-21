use maimai_app::rankings::{
    B50Report, B50ReportOptions, OutputMode, RankWindow, RankingResponse, SortOrder,
};
use maimai_storage::{RankingJobStatus, RankingNamespace};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use super::{
    RankingDispatcher, convert,
    dto::{B50JobArgs, B50MemberArgs, B50RankAtArgs, B50ReportArgs},
    error::RankingToolError,
    format,
};
use crate::{DispatchError, ToolOutput};

impl RankingDispatcher {
    pub(super) async fn b50_report(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: B50ReportArgs = super::deserialize(arguments)?;
        let group = convert::group_id(args.group_id.clone())?;
        let options = report_options(&args, SortOrder::Ascending)?;
        let refresh_args = args.refresh();
        let response = self
            .service
            .b50_report_with_client(
                group,
                options,
                args.force_refresh.unwrap_or(false),
                convert::refresh(&refresh_args)?,
                OffsetDateTime::now_utc(),
                convert::napcat_client(self.service.napcat(), &refresh_args)?,
            )
            .await
            .map_err(RankingToolError::from)?;
        self.b50_response(response, options).await
    }

    pub(super) async fn b50_job_status(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: B50JobArgs = super::deserialize(arguments)?;
        let group = convert::group_id(args.group_id.clone())?;
        let now = OffsetDateTime::now_utc();
        let job = self
            .service
            .job_status(RankingNamespace::B50, &group, now)
            .await
            .map_err(RankingToolError::from)?;
        if job
            .as_ref()
            .is_some_and(|job| job.status == RankingJobStatus::Completed)
        {
            let options = convert::b50_options(convert::B50OptionInput {
                sort_by: args.sort_by,
                sort_order: args.sort_order,
                output_mode: args.output_mode,
                rating_min: args.rating_min,
                rating_max: args.rating_max,
                fit_min: args.fit_index_min,
                fit_max: args.fit_index_max,
                window: convert::window(args.output_limit, None, None)?,
                default_order: SortOrder::Ascending,
            })?;
            let report = self
                .service
                .b50_cached_report(&group, options, now)
                .await
                .map_err(RankingToolError::from)?;
            let mut structured = b50_report_value(&report, "completed");
            if let Value::Object(object) = &mut structured {
                object.insert(
                    "job".to_owned(),
                    job.as_ref().map(format::job_value).unwrap_or(Value::Null),
                );
            }
            return super::output(
                format!(
                    "后台刷新已完成。\n\n{}",
                    format::b50_report(&report, self.display_offset)
                ),
                structured,
            );
        }
        let status = self
            .service
            .cache_status(RankingNamespace::B50, &group, now)
            .await
            .map_err(RankingToolError::from)?;
        super::output(
            format::job_status_text(job.as_ref(), &status, self.display_offset),
            json!({
                "groupId": group.as_str(),
                "sortOrder": args.sort_order.unwrap_or_else(|| "asc".to_owned()),
                "outputMode": args.output_mode.unwrap_or_else(|| "rating".to_owned()),
                "ratingMin": args.rating_min,
                "ratingMax": args.rating_max,
                "outputLimit": args.output_limit,
                "cache": format::cache_value(&status),
                "job": job.as_ref().map(format::job_value),
                "data": Value::Null,
            }),
        )
    }

    pub(super) async fn b50_member_rank(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: B50MemberArgs = super::deserialize(arguments)?;
        let (qq, group) = self
            .service
            .resolve_member(
                convert::optional_qq(args.qq.clone())?,
                args.target.as_deref(),
                convert::optional_group_id(args.group_id.clone())?,
            )
            .await
            .map_err(RankingToolError::from)?;
        let refresh_args = args.refresh();
        let now = OffsetDateTime::now_utc();
        if let Some(launch) = self
            .service
            .ensure_cache_with_client(
                RankingNamespace::B50,
                group.clone(),
                args.force_refresh.unwrap_or(false),
                convert::refresh(&refresh_args)?,
                now,
                convert::napcat_client(self.service.napcat(), &refresh_args)?,
            )
            .await
            .map_err(RankingToolError::from)?
        {
            return super::output(
                format::b50_member_started(&launch, &qq, self.display_offset),
                json!({
                    "groupId": group.as_str(), "qq": qq.as_str(),
                    "outputMode": args.output_mode.as_deref().unwrap_or("rating"),
                    "contextSize": args.context_size.unwrap_or(3),
                    "cache": format::cache_value(&launch.cache),
                    "job": format::job_value(&launch.job),
                    "cacheRefreshReason": format::reason(launch.reason),
                    "text": format::b50_member_started(&launch, &qq, self.display_offset),
                    "data": Value::Null,
                }),
            );
        }
        let context = convert::context(args.context_size)?;
        let result = self
            .service
            .b50_member_rank_cached(&group, qq.clone(), context, now)
            .await
            .map_err(RankingToolError::from)?;
        let output_mode = output_mode(args.output_mode)?;
        let text = format::b50_member(&result, output_mode, self.display_offset);
        let target = result
            .target
            .as_ref()
            .map(|entry| format::b50_entry_value(entry, group.as_str()));
        let context_value = result
            .context
            .iter()
            .map(|row| ranked_b50_row(row, group.as_str()))
            .collect::<Vec<_>>();
        super::output(
            text.clone(),
            json!({
                "groupId": group.as_str(), "qq": qq.as_str(), "found": result.found,
                "member": target, "rank": {
                    "rankDesc": result.rank_desc, "rankAsc": result.rank_asc,
                    "totalRanked": result.total_ranked,
                    "higherCount": result.rank_desc.map(|rank| rank.saturating_sub(1)),
                    "lowerCount": result.rank_desc.map(|rank| result.total_ranked.saturating_sub(rank)),
                },
                "context": context_value, "outputMode": output_name(output_mode),
                "contextSize": context.get(), "cache": format::cache_value(&result.cache),
                "cacheRefreshReason": "hit", "text": text,
                "data": {"target": target, "context": context_value},
            }),
        )
    }

    pub(super) async fn b50_rank_at(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: B50RankAtArgs = super::deserialize(arguments)?;
        let group = convert::group_id(args.group_id.clone())?;
        let rank = args
            .rank
            .filter(|rank| *rank > 0)
            .ok_or_else(|| RankingToolError::invalid("rank 必须是正整数。"))?;
        let order = match args.sort_order.as_deref() {
            None | Some("desc") => SortOrder::Descending,
            Some("asc") => SortOrder::Ascending,
            _ => return Err(RankingToolError::invalid("sortOrder 必须是 asc 或 desc。").into()),
        };
        let refresh_args = args.refresh();
        let options = convert::b50_options(convert::B50OptionInput {
            sort_by: None,
            sort_order: args.sort_order.clone(),
            output_mode: args.output_mode.clone(),
            rating_min: args.rating_min,
            rating_max: args.rating_max,
            fit_min: args.fit_index_min,
            fit_max: args.fit_index_max,
            window: RankWindow::default(),
            default_order: SortOrder::Descending,
        })?;
        let now = OffsetDateTime::now_utc();
        if let Some(launch) = self
            .service
            .ensure_cache_with_client(
                RankingNamespace::B50,
                group.clone(),
                args.force_refresh.unwrap_or(false),
                convert::refresh(&refresh_args)?,
                now,
                convert::napcat_client(self.service.napcat(), &refresh_args)?,
            )
            .await
            .map_err(RankingToolError::from)?
        {
            let text = format::b50_rank_at_started(&launch, rank, order, self.display_offset);
            return super::output(
                text.clone(),
                json!({
                    "groupId": group.as_str(), "rank": rank, "sortOrder": order_name(order),
                    "outputMode": output_name(options.output), "ratingMin": args.rating_min,
                    "ratingMax": args.rating_max, "cache": format::cache_value(&launch.cache),
                    "job": format::job_value(&launch.job), "cacheRefreshReason": format::reason(launch.reason),
                    "text": text, "data": Value::Null,
                }),
            );
        }
        let report = self
            .service
            .b50_cached_report(&group, options, now)
            .await
            .map_err(RankingToolError::from)?;
        let row = report.rows.get(rank - 1).cloned();
        let Some(row) = row else {
            let text = format::b50_rank_at_missing(group.as_str(), rank, order);
            return super::output(
                text.clone(),
                json!({
                    "groupId": group.as_str(), "rank": rank, "sortOrder": order_name(order),
                    "found": false, "outputMode": output_name(options.output), "matchedCount": report.matched_count,
                    "cache": format::cache_value(&report.cache), "cacheRefreshReason": "hit", "text": text,
                    "data": Value::Null,
                }),
            );
        };
        let member = self
            .service
            .b50_member_rank_cached(
                &group,
                row.entry.member.qq.clone(),
                convert::context(Some(0))?,
                now,
            )
            .await
            .map_err(RankingToolError::from)?;
        let text = format::b50_rank_at(&row, &member, order, options.output, self.display_offset);
        super::output(
            text.clone(),
            json!({
                "groupId": group.as_str(), "rank": rank, "sortOrder": order_name(order), "found": true,
                "member": format::b50_entry_value(&row.entry, group.as_str()),
                "rankInfo": {"requestedRank": rank, "sortOrder": order_name(order),
                    "matchedCount": report.matched_count, "rankDesc": member.rank_desc,
                    "rankAsc": member.rank_asc, "totalRanked": member.total_ranked},
                "outputMode": output_name(options.output), "matchedCount": report.matched_count,
                "cache": format::cache_value(&report.cache), "cacheRefreshReason": "hit", "text": text,
                "data": {"target": format::b50_entry_value(&row.entry, group.as_str())},
            }),
        )
    }

    async fn b50_response(
        &self,
        response: RankingResponse<B50Report>,
        options: B50ReportOptions,
    ) -> Result<ToolOutput, DispatchError> {
        match response {
            RankingResponse::Ready(report) => {
                let text = format::b50_report(&report, self.display_offset);
                super::output(
                    text.clone(),
                    with_text(b50_report_value(&report, "hit"), text),
                )
            }
            RankingResponse::Started(launch) => {
                let text = format::b50_started(&launch, self.display_offset);
                super::output(
                    text.clone(),
                    json!({
                        "groupId": launch.cache.group_id.as_str(), "sortBy": b50_sort_name(options),
                        "sortOrder": order_name(options.order), "outputMode": output_name(options.output),
                        "ratingMin": options.rating_min, "ratingMax": options.rating_max,
                        "outputLimit": options.window.limit, "startRank": options.window.start, "endRank": options.window.end,
                        "cache": format::cache_value(&launch.cache), "job": format::job_value(&launch.job),
                        "cacheRefreshReason": format::reason(launch.reason), "text": text, "data": Value::Null,
                    }),
                )
            }
        }
    }
}

fn report_options(
    args: &B50ReportArgs,
    default: SortOrder,
) -> Result<B50ReportOptions, RankingToolError> {
    convert::b50_options(convert::B50OptionInput {
        sort_by: args.sort_by.clone(),
        sort_order: args.sort_order.clone(),
        output_mode: args.output_mode.clone(),
        rating_min: args.rating_min,
        rating_max: args.rating_max,
        fit_min: args.fit_index_min.clone(),
        fit_max: args.fit_index_max.clone(),
        window: convert::window(args.output_limit, args.start_rank, args.end_rank)?,
        default_order: default,
    })
}

fn b50_report_value(report: &B50Report, reason: &str) -> Value {
    json!({
        "groupId": report.cache.group_id.as_str(), "sortBy": b50_sort_name(report.options),
        "sortOrder": order_name(report.options.order), "outputMode": output_name(report.options.output),
        "ratingMin": report.options.rating_min, "ratingMax": report.options.rating_max,
        "outputLimit": report.options.window.limit, "startRank": report.options.window.start,
        "endRank": report.options.window.end, "cache": format::cache_value(&report.cache),
        "cacheRefreshReason": reason,
        "data": {"groupId": report.cache.group_id.as_str(),
            "fetchedAt": report.cache.fetched_at.map(format::timestamp),
            "memberCount": report.cache.member_count, "successCount": report.cache.success_count,
            "failureCount": report.cache.failure_count, "skippedCount": report.cache.skipped_count,
            "results": report.all_entries.iter().map(|entry| format::b50_entry_value(entry, report.cache.group_id.as_str())).collect::<Vec<_>>()},
    })
}

fn ranked_b50_row(row: &maimai_app::rankings::B50Row, group: &str) -> Value {
    let mut value = format::b50_entry_value(&row.entry, group);
    if let Value::Object(object) = &mut value {
        object.insert("_rank".to_owned(), json!(row.rank));
    }
    value
}

fn with_text(mut value: Value, text: String) -> Value {
    if let Value::Object(object) = &mut value {
        object.insert("text".to_owned(), Value::String(text));
    }
    value
}

fn output_mode(value: Option<String>) -> Result<OutputMode, RankingToolError> {
    match value.as_deref().unwrap_or("rating") {
        "rating" => Ok(OutputMode::Rating),
        "detail" => Ok(OutputMode::Detail),
        _ => Err(RankingToolError::invalid(
            "outputMode 必须是 rating 或 detail。",
        )),
    }
}

const fn output_name(value: OutputMode) -> &'static str {
    match value {
        OutputMode::Rating => "rating",
        OutputMode::Detail => "detail",
    }
}

const fn order_name(value: SortOrder) -> &'static str {
    match value {
        SortOrder::Ascending => "asc",
        SortOrder::Descending => "desc",
    }
}

const fn b50_sort_name(value: B50ReportOptions) -> &'static str {
    match value.sort {
        maimai_app::rankings::B50Sort::Rating => "rating",
        maimai_app::rankings::B50Sort::FitIndex => "fitIndex",
    }
}
