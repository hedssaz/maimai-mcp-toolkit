use std::str::FromStr;

use maimai_app::rankings::CacheStatus;
use maimai_storage::{
    CachedB50Entry, CachedChart, RankingJob, RankingJobStatus, RankingMember, RankingNamespace,
    RankingRefreshReason,
};
use serde_json::{Number, Value, json};
use time::{OffsetDateTime, UtcOffset};

pub fn timestamp(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
    )
}

pub fn display_time(value: Option<OffsetDateTime>, offset: UtcOffset, missing: &str) -> String {
    let Some(value) = value else {
        return missing.to_owned();
    };
    let value = value.to_offset(offset);
    let seconds = offset.whole_seconds();
    let sign = if seconds < 0 { '-' } else { '+' };
    let absolute = seconds.unsigned_abs();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {sign}{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
        absolute / 3600,
        (absolute % 3600) / 60,
    )
}

pub fn cache_value(status: &CacheStatus) -> Value {
    let scope = match status.namespace {
        RankingNamespace::B50 => "full_group_detailed_b50",
        RankingNamespace::SongScore => "group_members_with_full_records",
    };
    json!({
        "groupId": status.group_id.as_str(),
        "feature": status.namespace.as_str(),
        "cacheExists": status.exists,
        "fresh": status.fresh,
        "ageSeconds": status.age_seconds,
        "nextResetAt": timestamp(status.next_reset_at),
        "fetchedAt": status.fetched_at.map(timestamp),
        "memberCount": status.member_count,
        "successCount": status.success_count,
        "failureCount": status.failure_count,
        "skippedCount": status.skipped_count,
        "cacheHitCount": status.cache_hit_count,
        "sharedFetchCount": status.shared_fetch_count,
        "cacheScope": scope,
        "containsDetailedB50": status.namespace == RankingNamespace::B50 && status.exists,
        "job": status.job.as_ref().map(job_value),
    })
}

pub fn job_value(job: &RankingJob) -> Value {
    json!({
        "jobId": format!("{}:{}:{}", job.namespace.as_str(), job.group_id.as_str(), job.generation),
        "feature": job.namespace.as_str(),
        "groupId": job.group_id.as_str(),
        "generation": job.generation,
        "status": job.status.as_str(),
        "startedAt": timestamp(job.started_at),
        "finishedAt": job.finished_at.map(timestamp),
        "refreshReason": reason(job.refresh_reason),
        "message": job.message,
        "processedCount": job.progress.processed_count,
        "totalCount": job.progress.total_count,
        "cachedCount": job.progress.cached_count,
        "skippedCount": job.progress.skipped_count,
        "transientFailureCount": job.progress.transient_failure_count,
        "currentQq": job.progress.current_qq.as_ref().map(maimai_core::QqId::as_str),
        "memberCount": job.member_count,
        "successCount": job.success_count,
        "error": job.error.as_ref().map(|error| json!({
            "code": error.code.as_str(),
            "message": error.message,
            "status": error.status,
            "body": error.body,
        })),
    })
}

pub fn cache_status_text(status: &CacheStatus, offset: UtcOffset) -> String {
    if !status.exists {
        return format!("群 {} 没有缓存。", status.group_id.as_str());
    }
    let freshness = if status.fresh {
        "未过期"
    } else {
        "已过期"
    };
    let scope = match status.namespace {
        RankingNamespace::B50 => "完整 B50 歌曲明细",
        RankingNamespace::SongScore => "全群完整成绩",
    };
    format!(
        "群 {} 缓存存在，{}。\n生成时间: {}\n年龄: {} 秒\n群成员: {}，成功: {}，失败: {}\n跳过: {}，缓存范围: {}",
        status.group_id.as_str(),
        freshness,
        display_time(status.fetched_at, offset, "未知"),
        status.age_seconds.unwrap_or_default(),
        optional_count(status.member_count),
        optional_count(status.success_count),
        optional_count(status.failure_count),
        optional_count(status.skipped_count),
        scope,
    )
}

pub fn job_status_text(
    job: Option<&RankingJob>,
    status: &CacheStatus,
    offset: UtcOffset,
) -> String {
    let Some(job) = job else {
        return format!(
            "群 {} 当前没有后台刷新任务。\n缓存存在: {}，未过期: {}",
            status.group_id.as_str(),
            status.exists,
            status.fresh
        );
    };
    let mut lines = vec![
        format!(
            "群 {} 后台刷新任务状态: {}",
            status.group_id.as_str(),
            job.status.as_str()
        ),
        format!(
            "启动时间: {}",
            display_time(Some(job.started_at), offset, "未知")
        ),
        format!(
            "完成时间: {}",
            display_time(job.finished_at, offset, "未完成")
        ),
        format!("说明: {}", job.message),
    ];
    if job.status == RankingJobStatus::Running {
        lines.push(format!(
            "进度: {}/{}，已缓存: {}，跳过: {}，临时失败: {}",
            job.progress.processed_count,
            job.progress
                .total_count
                .map_or_else(|| "?".to_owned(), |value| value.to_string()),
            job.progress.cached_count,
            job.progress.skipped_count,
            job.progress.transient_failure_count,
        ));
    }
    if job.status == RankingJobStatus::Failed
        && let Some(error) = &job.error
    {
        lines.push(format!("错误: {}", error.message));
    }
    lines.join("\n")
}

pub fn ranking_member_value(member: &RankingMember, group_id: &str) -> Value {
    json!({
        "userId": member.qq.as_str(),
        "displayName": member.display_name,
        "nickname": member.nickname,
        "card": member.card,
        "identity": identity_value(member, group_id),
    })
}

pub fn identity_value(member: &RankingMember, group_id: &str) -> Value {
    json!({
        "qq": member.qq.as_str(),
        "qqNickname": member.nickname,
        "waterfishNickname": member.waterfish_nickname,
        "waterfishUsername": member.waterfish_username,
        "preferredGroup": {
            "groupId": group_id,
            "groupNickname": member.display_name,
            "card": member.card,
            "nickname": member.nickname,
        }
    })
}

pub fn b50_entry_value(entry: &CachedB50Entry, group_id: &str) -> Value {
    let ratio = fit_ratio(entry);
    let mut value = ranking_member_value(&entry.member, group_id);
    let Value::Object(ref mut object) = value else {
        return value;
    };
    object.insert("ok".to_owned(), Value::Bool(true));
    object.insert("rating".to_owned(), json!(entry.player.rating));
    object.insert("b50Rating".to_owned(), json!(entry.rating_breakdown.total));
    object.insert(
        "player".to_owned(),
        json!({
            "nickname": entry.player.nickname,
            "username": entry.player.username,
            "rating": entry.player.rating,
            "additionalRating": entry.player.additional_rating,
            "plate": entry.player.plate,
        }),
    );
    object.insert(
        "fitIndex".to_owned(),
        json!({
            "available": ratio.is_some(),
            "label": entry.fit_index.label.map(fit_label_cached),
            "virtualRating": entry.fit_index.b50.virtual_rating,
            "virtualRatio": ratio,
            "counted": entry.fit_index.b50.counted,
            "missing": entry.fit_index.b50.missing,
            "b50": fit_section_value(entry.fit_index.b50),
            "b35": fit_section_value(entry.fit_index.b35),
            "b15": fit_section_value(entry.fit_index.b15),
        }),
    );
    object.insert("b50".to_owned(), b50_value(entry));
    object.insert("error".to_owned(), Value::Null);
    value
}

fn fit_section_value(section: maimai_storage::CachedFitIndexSection) -> Value {
    json!({
        "virtualRating": section.virtual_rating,
        "virtualRatio": section.virtual_ratio_percent.map(|ratio| ratio.numerator as f64 / ratio.denominator as f64),
        "weightedAvgFitDelta": section.weighted_average_delta.map(|ratio| ratio.numerator as f64 / ratio.denominator as f64),
        "counted": section.counted,
        "missing": section.missing,
        "totalRa": section.total_rating,
    })
}

pub fn chart_value(chart: &CachedChart) -> Value {
    json!({
        "title": chart.title,
        "type": generation(chart.key.generation()),
        "level": chart.level,
        "levelLabel": difficulty(chart.key.difficulty()),
        "levelIndex": difficulty_index(chart.key.difficulty()),
        "ds": chart.constant.map(|value| value.value().normalize().to_string()),
        "achievements": chart.achievements.map(achievement_value),
        "dxScore": chart.dx_score,
        "fc": chart.full_combo,
        "fs": chart.full_sync,
        "ra": chart.rating,
        "rate": chart.grade,
        "songId": chart.key.song().value(),
    })
}

fn achievement_value(value: maimai_core::AchievementRate) -> Value {
    let units = value.ten_thousandths();
    Number::from_str(&format!("{}.{:04}", units / 10_000, units % 10_000))
        .map_or(Value::Null, Value::Number)
}

fn b50_value(entry: &CachedB50Entry) -> Value {
    let b35 = entry
        .charts
        .iter()
        .filter(|chart| chart.section == maimai_storage::B50Section::B35)
        .map(|chart| chart_value(&chart.chart))
        .collect::<Vec<_>>();
    let b15 = entry
        .charts
        .iter()
        .filter(|chart| chart.section == maimai_storage::B50Section::B15)
        .map(|chart| chart_value(&chart.chart))
        .collect::<Vec<_>>();
    json!({
        "player": {
            "nickname": entry.player.nickname,
            "username": entry.player.username,
            "rating": entry.player.rating,
        },
        "ratingBreakdown": {
            "sd": entry.rating_breakdown.b35,
            "dx": entry.rating_breakdown.b15,
            "total": entry.rating_breakdown.total,
        },
        "charts": {"sd": b35, "dx": b15},
    })
}

pub fn fit_ratio(entry: &CachedB50Entry) -> Option<f64> {
    let ratio = entry.fit_index.b50.virtual_ratio_percent?;
    let numerator = ratio.numerator as f64;
    let denominator = ratio.denominator as f64;
    Some(numerator / denominator)
}

fn fit_label_cached(value: maimai_storage::CachedFitIndexLabel) -> &'static str {
    match value {
        maimai_storage::CachedFitIndexLabel::ClearlyInflated => "明显虚高（水）",
        maimai_storage::CachedFitIndexLabel::SlightlyInflated => "略微虚高",
        maimai_storage::CachedFitIndexLabel::Balanced => "基本持平",
        maimai_storage::CachedFitIndexLabel::SlightlyDeflated => "略微虚低",
        maimai_storage::CachedFitIndexLabel::ClearlyDeflated => "明显虚低（硬实力）",
    }
}

pub fn fit_label(value: f64) -> &'static str {
    if value > 1.0 {
        "明显虚高（水）"
    } else if value > 0.2 {
        "略微虚高"
    } else if value < -1.0 {
        "明显虚低（硬实力）"
    } else if value < -0.2 {
        "略微虚低"
    } else {
        "基本持平"
    }
}

pub const fn difficulty(value: maimai_core::Difficulty) -> &'static str {
    match value {
        maimai_core::Difficulty::Basic => "Basic",
        maimai_core::Difficulty::Advanced => "Advanced",
        maimai_core::Difficulty::Expert => "Expert",
        maimai_core::Difficulty::Master => "Master",
        maimai_core::Difficulty::ReMaster => "Re:Master",
        maimai_core::Difficulty::Utage => "Utage",
    }
}

pub const fn difficulty_index(value: maimai_core::Difficulty) -> Option<u8> {
    match value {
        maimai_core::Difficulty::Basic => Some(0),
        maimai_core::Difficulty::Advanced => Some(1),
        maimai_core::Difficulty::Expert => Some(2),
        maimai_core::Difficulty::Master => Some(3),
        maimai_core::Difficulty::ReMaster => Some(4),
        maimai_core::Difficulty::Utage => None,
    }
}

pub const fn generation(value: maimai_core::ChartGeneration) -> &'static str {
    match value {
        maimai_core::ChartGeneration::Deluxe => "DX",
        maimai_core::ChartGeneration::Standard => "SD",
        maimai_core::ChartGeneration::UtageOnePlayer
        | maimai_core::ChartGeneration::UtageTwoPlayer => "UTAGE",
    }
}

pub const fn reason(value: RankingRefreshReason) -> &'static str {
    match value {
        RankingRefreshReason::Miss => "miss",
        RankingRefreshReason::Stale => "stale",
        RankingRefreshReason::ForceRefresh => "forceRefresh",
    }
}

fn optional_count(value: Option<u32>) -> String {
    value.map_or_else(|| "未知".to_owned(), |value| value.to_string())
}
