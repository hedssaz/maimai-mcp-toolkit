use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    DisplayOffset,
    dto::{CacheDto, IdentityDto, JobDto, ResolveDto},
    error::IdentityToolError,
};

pub fn cache_status(
    status: &CacheDto,
    prefix: Option<&str>,
    display_offset: DisplayOffset,
) -> Result<String, IdentityToolError> {
    let mut lines = Vec::with_capacity(5);
    if let Some(prefix) = prefix {
        lines.push(prefix.to_owned());
    }
    lines.extend([
        format!(
            "缓存存在: {}，未过期: {}",
            legacy_bool(status.cache_exists),
            legacy_bool(status.fresh)
        ),
        format!(
            "生成时间: {}",
            display_time(status.fetched_at.as_deref(), "未知", display_offset)?
        ),
        format!(
            "好友: {}，群: {}，群成员记录: {}，唯一 QQ: {}",
            status.stats.friend_count,
            status.stats.group_count,
            status.stats.group_member_rows,
            status.stats.unique_users
        ),
        "好友侧仅保存 QQ 昵称，不保存备注。".to_owned(),
    ]);
    Ok(lines.join("\n"))
}

pub fn job_started(
    job: &JobDto,
    display_offset: DisplayOffset,
) -> Result<String, IdentityToolError> {
    Ok([
        "QQ 身份缓存刷新已启动。".to_owned(),
        format!(
            "状态: {}，原因: {}，启动时间: {}",
            job.status,
            job.refresh_reason,
            display_time(Some(&job.started_at), "未知", display_offset)?
        ),
        "稍后调用 qq_identity_job_status 查看进度；完成后可用 resolve_qq_identity 反查昵称。"
            .to_owned(),
    ]
    .join("\n"))
}

pub fn job_status(
    job: Option<&JobDto>,
    display_offset: DisplayOffset,
) -> Result<String, IdentityToolError> {
    let Some(job) = job else {
        return Ok("当前没有 QQ 身份缓存刷新任务。".to_owned());
    };
    let mut lines = vec![
        format!("QQ 身份缓存刷新任务状态: {}", job.status),
        format!(
            "启动时间: {}",
            display_time(Some(&job.started_at), "未知", display_offset)?
        ),
        format!(
            "完成时间: {}",
            display_time(job.finished_at.as_deref(), "未完成", display_offset)?
        ),
        format!("说明: {}", job.message),
    ];
    if job.status == "running" {
        let total = job
            .total_groups
            .map_or_else(|| "?".to_owned(), |value| value.to_string());
        lines.push(format!(
            "进度: {}/{} 个群",
            job.processed_groups.unwrap_or(0),
            total
        ));
        if let Some(unique_users) = job.unique_users {
            lines.push(format!("当前唯一 QQ: {unique_users}"));
        }
    }
    if job.status == "completed"
        && let Some(stats) = job.stats.as_ref()
    {
        lines.push(format!(
            "统计: 好友 {}，群 {}，唯一 QQ {}",
            stats.friend_count, stats.group_count, stats.unique_users
        ));
    }
    if job.status == "failed"
        && let Some(error) = job.error.as_ref()
    {
        lines.push(format!("错误: {}", error.message));
    }
    Ok(lines.join("\n"))
}

pub fn resolve(result: &ResolveDto) -> String {
    if result.matches.is_empty() {
        return format!("没有从 QQ 身份缓存中找到：{}", result.query);
    }
    let mut lines = vec![
        format!(
            "找到 {} 个候选{}:",
            result.matches.len(),
            if result.ambiguous {
                "（存在重名，请让用户选择 QQ）"
            } else {
                ""
            }
        ),
        "| 序号 | QQ | QQ昵称 | 群昵称 | 水鱼昵称 | 匹配字段 |".to_owned(),
        "| --- | --- | --- | --- | --- | --- |".to_owned(),
    ];
    for (index, candidate) in result.matches.iter().enumerate() {
        let group_name = candidate
            .identity
            .preferred_group
            .as_ref()
            .map(|group| group.group_nickname.as_str())
            .or_else(|| {
                candidate
                    .identity
                    .groups
                    .first()
                    .map(|group| group.group_nickname.as_str())
            })
            .unwrap_or_default();
        let qq_name = candidate
            .identity
            .qq_nickname
            .as_deref()
            .or(candidate.identity.friend_nickname.as_deref())
            .unwrap_or_default();
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} |",
            index + 1,
            candidate.identity.qq,
            escape_table(qq_name),
            escape_table(group_name),
            escape_table(
                candidate
                    .identity
                    .waterfish_nickname
                    .as_deref()
                    .unwrap_or_default()
            ),
            escape_table(&candidate.matched_fields.join(", "))
        ));
    }
    lines.join("\n")
}

pub fn identity(identity: Option<&IdentityDto>, qq: &str) -> String {
    let Some(identity) = identity else {
        return format!("QQ {qq} 不在当前 QQ 身份缓存中。");
    };
    let mut lines = vec![
        format!("QQ: {}", identity.qq),
        format!(
            "QQ昵称: {}",
            identity
                .qq_nickname
                .as_deref()
                .or(identity.friend_nickname.as_deref())
                .unwrap_or("未知")
        ),
        format!(
            "水鱼昵称: {}",
            identity.waterfish_nickname.as_deref().unwrap_or("未知")
        ),
    ];
    if let Some(group) = identity.preferred_group.as_ref() {
        let suffix = group
            .group_name
            .as_ref()
            .map_or_else(String::new, |name| format!("（{name}）"));
        lines.push(format!(
            "当前群昵称: {}{}",
            if group.group_nickname.is_empty() {
                "未知"
            } else {
                &group.group_nickname
            },
            suffix
        ));
    }
    if !identity.groups.is_empty() {
        lines.push(format!("所在群记录: {} 个", identity.groups.len()));
    }
    lines.join("\n")
}

pub fn refresh_not_started(
    cache: &CacheDto,
    job: Option<&JobDto>,
    display_offset: DisplayOffset,
) -> Result<String, IdentityToolError> {
    if job.is_some_and(|job| job.status == "running") {
        return Ok(format!(
            "QQ 身份缓存刷新仍在进行，未启动新任务。\n\n{}",
            job_status(job, display_offset)?
        ));
    }
    let prefix = match job.map(|job| job.status.as_str()) {
        Some("completed") => {
            "最近一次 QQ 身份缓存刷新已完成；当前缓存仍在 1 天有效期内，未重新拉取。"
        }
        Some("failed") => "最近一次 QQ 身份缓存刷新失败；当前缓存仍在 1 天有效期内，未重新拉取。",
        _ => "QQ 身份缓存仍在 1 天有效期内，未重新拉取。",
    };
    let mut text = cache_status(cache, Some(prefix), display_offset)?;
    if job.is_some() {
        text.push_str("\n\n最近刷新任务:\n");
        text.push_str(&job_status(job, display_offset)?);
    }
    Ok(text)
}

fn display_time(
    value: Option<&str>,
    missing: &str,
    display_offset: DisplayOffset,
) -> Result<String, IdentityToolError> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(missing.to_owned());
    };
    let parsed =
        OffsetDateTime::parse(value, &Rfc3339).map_err(|_| IdentityToolError::internal())?;
    let local = parsed.to_offset(display_offset.get());
    let offset_seconds = display_offset.get().whole_seconds();
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let absolute = offset_seconds.unsigned_abs();
    let offset_hours = absolute / 3_600;
    let offset_minutes = absolute % 3_600 / 60;
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {sign}{offset_hours:02}:{offset_minutes:02}",
        local.year(),
        u8::from(local.month()),
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
    ))
}

fn escape_table(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

const fn legacy_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}
