use serde_json::Value;

use super::value::{array, fmt};

pub(super) fn render(result: &Value) -> String {
    if result.get("background").and_then(Value::as_bool) == Some(true) {
        return render_started(result);
    }
    let mut lines = vec![
        format!(
            "源刷新: force={} check_only={} ttl_days={}",
            fmt(result.get("force"), "-"),
            fmt(result.get("check_only"), "-"),
            fmt(result.get("ttl_days"), "-")
        ),
        format!("应刷新: {}", joined(result.get("due_sources"))),
        format!("已刷新: {}", joined(result.get("refreshed_sources"))),
        format!("已跳过: {}", joined(result.get("skipped_sources"))),
        format!("失败: {}", joined(result.get("failed_sources"))),
    ];
    if let Some(sources) = result.get("sources").and_then(Value::as_object) {
        for (source, status) in sources {
            lines.push(format!(
                "- {source}: expired={} age_days={} mtime={}",
                fmt(status.get("expired"), "-"),
                fmt(status.get("age_days"), "-"),
                fmt(status.get("mtime"), "-")
            ));
        }
    }
    for operation in array(result.get("commands")) {
        if let Some(error) = operation.get("error").and_then(Value::as_str) {
            lines.push(format!("! {}: {error}", fmt(operation.get("source"), "-")));
        } else {
            lines.push(format!(
                "+ {}: returncode={} duration={}s",
                fmt(operation.get("source"), "-"),
                fmt(operation.get("returncode"), "-"),
                fmt(operation.get("duration_seconds"), "-")
            ));
        }
    }
    lines.join("\n")
}

pub(super) fn render_job(result: &Value) -> String {
    if let Some(error) = result.get("error").and_then(Value::as_str) {
        return format!("错误: {error}");
    }
    let mut lines = vec![
        format!(
            "刷新进度: status={} completed={}/{}",
            fmt(result.get("status"), "unknown"),
            fmt(result.get("completedSources"), "?"),
            fmt(result.get("totalSources"), "?")
        ),
        format!("成功: {}", joined(result.get("succeededSources"))),
        format!("失败: {}", joined(result.get("failedSources"))),
        format!("message: {}", fmt(result.get("message"), "")),
    ];
    if let Some(sources) = result.get("sources").and_then(Value::as_object) {
        for (source, operation) in sources {
            let tag = if operation.get("returncode").and_then(Value::as_i64) == Some(0) {
                "✅"
            } else {
                "❌"
            };
            lines.push(format!(
                "  {tag} {source}: rc={} dur={}s",
                fmt(operation.get("returncode"), "-"),
                fmt(operation.get("duration_seconds"), "-")
            ));
        }
    }
    lines.join("\n")
}

fn render_started(result: &Value) -> String {
    let due = fmt(result.get("dueSources"), "?");
    let mut lines = vec![
        format!(
            "后台刷新: jobId={} status={}",
            fmt(result.get("jobId"), "-"),
            fmt(result.get("status"), "unknown")
        ),
        format!(
            "过期 {due}/{} 源，刷新中...",
            fmt(result.get("totalSources"), "?")
        ),
    ];
    if let Some(states) = result.get("sourceStates").and_then(Value::as_object) {
        for (source, state) in states {
            let tag = if state.get("expired").and_then(Value::as_bool) == Some(true) {
                "🔄"
            } else {
                "✅"
            };
            lines.push(format!(
                "  {tag} {source} ({}): 过期={} 年龄={}天",
                fmt(state.get("label"), source),
                fmt(state.get("expired"), "-"),
                fmt(state.get("age_days"), "-")
            ));
        }
    }
    lines.push(format!("> {}", fmt(result.get("message"), "")));
    lines.join("\n")
}

fn joined(value: Option<&Value>) -> String {
    let joined = array(value)
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if joined.is_empty() {
        "-".to_owned()
    } else {
        joined
    }
}
