use std::time::Duration;

use serde_json::{Value, json};
use tokio::task::JoinSet;

use maimai_core::QqId;

use crate::{DispatchError, ToolOutput};

use super::{ScoreQueryHandler, clock};
use crate::score_queries::{
    dto::{B50Args, BatchArgs},
    error::ScoreQueryToolError,
    format,
};

impl ScoreQueryHandler {
    pub(super) async fn query_b50_batch(
        &self,
        args: BatchArgs,
    ) -> Result<ToolOutput, DispatchError> {
        validate(&args)?;
        let (_, requested_at) = clock()?;
        let delay = Duration::from_millis(args.query_delay_ms.unwrap_or(250));
        let concurrency = args.max_concurrency.unwrap_or(1);
        let mut results = vec![None; args.qqs.len()];
        let mut tasks = JoinSet::new();
        let mut next = 0usize;
        while next < args.qqs.len() || !tasks.is_empty() {
            while next < args.qqs.len() && tasks.len() < concurrency {
                if delay > Duration::ZERO && next > 0 {
                    tokio::time::sleep(delay).await;
                }
                let index = next;
                let qq = args.qqs[index].clone();
                let handler = self.clone();
                let query = b50_args(&args, qq.clone());
                let include_summary = args.include_summaries;
                tasks.spawn(async move {
                    let result = handler.execute_b50(query, false, true).await;
                    (index, batch_item(qq, result, include_summary))
                });
                next += 1;
            }
            if let Some(joined) = tasks.join_next().await {
                let (index, item) = joined.map_err(|_| ScoreQueryToolError::internal())?;
                results[index] = Some(item);
            }
        }
        let results = results
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(ScoreQueryToolError::internal)?;
        let success = results
            .iter()
            .filter(|item| item["ok"].as_bool() == Some(true))
            .count();
        let structured = json!({
            "source":"diving-fish",
            "endpoint":"/query/player",
            "requestedAt":requested_at,
            "counts":{
                "requested":results.len(),
                "success":success,
                "failure":results.len()-success,
            },
            "results":results,
        });
        Ok(ToolOutput::text(format::batch(&structured)).with_structured_content(structured))
    }
}

fn validate(args: &BatchArgs) -> Result<(), ScoreQueryToolError> {
    if args.qqs.is_empty() || args.qqs.len() > 500 {
        return Err(ScoreQueryToolError::invalid(
            "qqs 必须包含 1 到 500 个 QQ。",
        ));
    }
    for qq in &args.qqs {
        QqId::new(qq.clone())
            .map_err(|_| ScoreQueryToolError::invalid("qqs 必须全部是合法 QQ 数字字符串。"))?;
    }
    let delay = args.query_delay_ms.unwrap_or(250);
    if delay > 10_000 {
        return Err(ScoreQueryToolError::invalid(
            "queryDelayMs 必须是 0 到 10000 之间的整数。",
        ));
    }
    if !(1..=20).contains(&args.max_concurrency.unwrap_or(1)) {
        return Err(ScoreQueryToolError::invalid(
            "maxConcurrency 必须是 1 到 20 之间的整数。",
        ));
    }
    Ok(())
}

fn b50_args(args: &BatchArgs, qq: String) -> B50Args {
    B50Args {
        qq: Some(qq),
        top_n: args.top_n,
        section: args.section.clone(),
        include_raw: args.include_raw,
        timeout_ms: args.timeout_ms,
        include_chart_metadata: Some(
            args.include_chart_metadata
                .unwrap_or(args.include_summaries || filters_active(args)),
        ),
        group_id: args.group_id.clone(),
        sort_by: args.sort_by.clone(),
        sort_order: args.sort_order.clone(),
        level: args.level.clone(),
        difficulty: args.difficulty.clone(),
        ds_min: args.ds_min.clone(),
        ds_max: args.ds_max.clone(),
        achievement_min: args.achievement_min.clone(),
        achievement_max: args.achievement_max.clone(),
        ra_min: args.ra_min.clone(),
        ra_max: args.ra_max.clone(),
        fit_diff_min: args.fit_diff_min.clone(),
        fit_diff_max: args.fit_diff_max.clone(),
        fit_delta_min: args.fit_delta_min.clone(),
        fit_delta_max: args.fit_delta_max.clone(),
        fit_label: args.fit_label.clone(),
        ..B50Args::default()
    }
}

fn filters_active(args: &BatchArgs) -> bool {
    args.sort_by
        .as_deref()
        .is_some_and(|value| value != "default")
        || args
            .sort_order
            .as_deref()
            .is_some_and(|value| value != "desc")
        || args.level.is_some()
        || args.difficulty.is_some()
        || args.ds_min.is_some()
        || args.ds_max.is_some()
        || args.achievement_min.is_some()
        || args.achievement_max.is_some()
        || args.ra_min.is_some()
        || args.ra_max.is_some()
        || args.fit_diff_min.is_some()
        || args.fit_diff_max.is_some()
        || args.fit_delta_min.is_some()
        || args.fit_delta_max.is_some()
        || args.fit_label.is_some()
}

fn batch_item(
    qq: String,
    result: Result<super::query::B50Execution, ScoreQueryToolError>,
    include_summary: bool,
) -> Value {
    match result {
        Ok(execution) => {
            let mut result = execution.structured;
            if let Some(preference) = result.get_mut("sourcePreference") {
                preference["explicitSource"] = Value::Bool(false);
            }
            let summary = include_summary.then_some(execution.text);
            let fit_index = summarize_fit_index(result.get("fitIndex"));
            json!({
                "qq":qq,
                "ok":true,
                "player":result["player"],
                "rating":result["player"]["rating"],
                "b50Rating":result["ratingBreakdown"]["total"],
                "fitIndex":fit_index,
                "result":result,
                "summary":summary,
                "error":Value::Null,
            })
        }
        Err(error) => {
            let error = error.structured();
            json!({"qq":qq,"ok":false,"player":Value::Null,"rating":Value::Null,
                "b50Rating":Value::Null,"fitIndex":Value::Null,"result":Value::Null,
                "summary":Value::Null,"error":error})
        }
    }
}

fn summarize_fit_index(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return Value::Null;
    };
    let b50 = &value["b50"];
    if b50["counted"].as_u64().unwrap_or(0) == 0 {
        return json!({"available":false,"label":"数据不足","virtualRating":Value::Null,
            "virtualRatio":Value::Null,"counted":0,"missing":b50["missing"]});
    }
    json!({
        "available":true,
        "label":value["label"],
        "virtualRating":b50["virtualRating"],
        "virtualRatio":b50["virtualRatio"],
        "counted":b50["counted"],
        "missing":b50["missing"],
    })
}
