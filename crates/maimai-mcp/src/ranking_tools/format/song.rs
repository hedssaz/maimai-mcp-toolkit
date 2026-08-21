use maimai_app::rankings::{
    RankingLaunch, SongMemberRank, SongReport, SongRow, SongSort, SortOrder,
};
use time::UtcOffset;

use super::value::{difficulty, display_time};

pub fn song_started(launch: &RankingLaunch, music_id: Option<u32>, offset: UtcOffset) -> String {
    let target = music_id.map_or_else(
        || "未指定曲目，本次只刷新/预热全群完整成绩缓存。".to_owned(),
        |music_id| format!("music_id={music_id}。"),
    );
    format!(
        "群 {} 单曲成绩缓存不存在/已过期/被要求刷新，已启动后台任务。\n任务状态: {}，启动时间: {}\n{}会按有界并发查询完整成绩。\n稍后调 group_song_score_job_status 看进度，或重新调 group_song_score_report 读结果。",
        launch.cache.group_id.as_str(),
        launch.job.status.as_str(),
        display_time(Some(launch.job.started_at), offset, "未知"),
        target,
    )
}

pub fn song_member_started(
    launch: &RankingLaunch,
    qq: &maimai_core::QqId,
    music_id: Option<u32>,
    offset: UtcOffset,
) -> String {
    format!(
        "群 {} QQ {} 在 music_id={} 的群内排名需要完整成绩缓存，已启动后台刷新。\n任务状态: {}，启动时间: {}\n稍后再次调用 group_song_score_member_rank 读取排名。",
        launch.cache.group_id.as_str(),
        qq,
        music_id.map_or_else(|| "?".to_owned(), |value| value.to_string()),
        launch.job.status.as_str(),
        display_time(Some(launch.job.started_at), offset, "未知"),
    )
}

pub fn song_cache_ready(status: &maimai_app::rankings::CacheStatus, offset: UtcOffset) -> String {
    format!(
        "群 {} 单曲成绩缓存已就绪。\n缓存状态: 本次使用一天内缓存，未重新拉取，缓存生成时间: {}\n群成员: {}，已缓存: {}，跳过: {}\n未指定 songQuery 或 musicId，本次只刷新/预热缓存，不生成单曲排行榜。",
        status.group_id.as_str(),
        display_time(status.fetched_at, offset, "未知"),
        status.member_count.unwrap_or_default(),
        status.success_count.unwrap_or_default(),
        status.skipped_count.unwrap_or_default(),
    )
}

pub fn song_report(report: &SongReport, offset: UtcOffset) -> String {
    let music_id = report
        .target
        .ids
        .iter()
        .find_map(|id| match id.value() {
            maimai_core::SongIdValue::Numeric(value)
                if id.namespace() == maimai_core::SongIdNamespace::DivingFish =>
            {
                Some(*value)
            }
            maimai_core::SongIdValue::Numeric(_) | maimai_core::SongIdValue::Text(_) => None,
        })
        .map_or_else(|| "?".to_owned(), |value| value.to_string());
    let mut lines = vec![
        format!(
            "群 {} 单曲成绩榜（music_id={}，{}）",
            report.cache.group_id.as_str(),
            music_id,
            report.target.title
        ),
        format!(
            "缓存状态: 本次使用一天内缓存，未重新拉取，缓存生成时间: {}",
            display_time(report.cache.fetched_at, offset, "未知")
        ),
        format!(
            "筛选: 难度={}, 谱面={}；排序: {} {}；匹配: {}，本次展示: {}",
            report.target.difficulty.map_or("不限", difficulty),
            report
                .target
                .deluxe
                .map_or("不限", |value| if value { "DX" } else { "SD" }),
            sort_label(report.sort),
            order_label(report.order),
            report.matched_count,
            report.rows.len(),
        ),
        format!(
            "群成员: {}，已缓存: {}，跳过: {}",
            report.cache.member_count.unwrap_or_default(),
            report.cache.success_count.unwrap_or_default(),
            report.cache.skipped_count.unwrap_or_default(),
        ),
    ];
    if report.rows.is_empty() {
        lines.extend([String::new(), "群内没有人有这首歌的成绩。".to_owned()]);
    } else {
        lines.extend([String::new(), table(&report.rows, None)]);
    }
    lines.join("\n")
}

pub fn song_member(rank: &SongMemberRank, offset: UtcOffset) -> String {
    let Some(row) = &rank.row else {
        return song_member_missing(rank, offset);
    };
    let record = &row.record;
    format!(
        "群 {} QQ {} 在 {} 的群内排名\n缓存生成时间: {}\n曲目: {} - {} / 定数 {}\n达成率: {}% / Rate: {} / FC: {} / FS: {} / DX Score: {} / ra: {}\n达成率倒序排名: {} / {}（前面 {} 人，后面 {} 人）\n达成率正序排名: {} / {}\n\n附近排名（达成率排名正序）:\n{}",
        rank.cache.group_id.as_str(),
        rank.qq,
        rank.target.title,
        display_time(rank.cache.fetched_at, offset, "未知"),
        record.title,
        difficulty(record.key.difficulty()),
        record.constant.map_or_else(
            || "None".to_owned(),
            |value| value.value().normalize().to_string()
        ),
        achievement(record.achievements),
        record.grade.as_deref().unwrap_or_default().to_uppercase(),
        record
            .full_combo
            .as_ref()
            .map(|value| value.as_str())
            .unwrap_or_default()
            .to_uppercase(),
        record
            .full_sync
            .as_ref()
            .map(|value| value.as_str())
            .unwrap_or_default()
            .to_uppercase(),
        record
            .dx_score
            .map_or_else(|| "None".to_owned(), |value| value.to_string()),
        record
            .rating
            .map_or_else(|| "None".to_owned(), |value| value.to_string()),
        rank.rank_desc.unwrap_or_default(),
        rank.total_ranked,
        rank.rank_desc.unwrap_or(1).saturating_sub(1),
        rank.total_ranked
            .saturating_sub(rank.rank_desc.unwrap_or_default()),
        rank.rank_asc.unwrap_or_default(),
        rank.total_ranked,
        table(&rank.context, Some(&rank.qq)),
    )
}

pub fn song_member_missing(rank: &SongMemberRank, offset: UtcOffset) -> String {
    format!(
        "群 {} QQ {} 在 {} 上没有可用成绩。\n可能原因：没玩过这首歌、对方隐私设置导致 records 拿不到、records 缓存里没有该曲。\n缓存生成时间: {}",
        rank.cache.group_id.as_str(),
        rank.qq,
        rank.target.title,
        display_time(rank.cache.fetched_at, offset, "未知"),
    )
}

fn table(rows: &[SongRow], target: Option<&maimai_core::QqId>) -> String {
    let mut lines = vec![
        "| 排名 | QQ | QQ昵称 | QQ群昵称 | 水鱼昵称 | 达成率 | Rate | FC | FS | DX Score | ra | 标记 |".to_owned(),
        "| --- | --- | --- | --- | --- | ---: | --- | --- | --- | ---: | ---: | --- |".to_owned(),
    ];
    for row in rows {
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {}% | {} | {} | {} | {} | {} | {} |",
            row.rank,
            row.qq,
            escape(row.member.nickname.as_deref().unwrap_or_default()),
            escape(&row.member.display_name),
            escape(row.member.waterfish_nickname.as_deref().unwrap_or_default()),
            achievement(row.record.achievements),
            row.record
                .grade
                .as_deref()
                .unwrap_or_default()
                .to_uppercase(),
            row.record
                .full_combo
                .as_ref()
                .map(|value| value.as_str())
                .unwrap_or_default()
                .to_uppercase(),
            row.record
                .full_sync
                .as_ref()
                .map(|value| value.as_str())
                .unwrap_or_default()
                .to_uppercase(),
            row.record
                .dx_score
                .map_or_else(String::new, |value| value.to_string()),
            row.record
                .rating
                .map_or_else(String::new, |value| value.to_string()),
            if target == Some(&row.qq) {
                "目标"
            } else {
                ""
            },
        ));
    }
    lines.join("\n")
}

fn achievement(value: Option<maimai_core::AchievementRate>) -> String {
    value.map_or_else(
        || "None".to_owned(),
        |value| {
            let units = value.ten_thousandths();
            let mut text = format!("{}.{:04}", units / 10_000, units % 10_000);
            while text.ends_with('0') && !text.ends_with(".0") {
                text.pop();
            }
            text
        },
    )
}

fn sort_label(value: SongSort) -> &'static str {
    match value {
        SongSort::Achievements => "achievements",
        SongSort::Rating => "ra",
        SongSort::DxScore => "dxScore",
    }
}

fn order_label(value: SortOrder) -> &'static str {
    match value {
        SortOrder::Ascending => "asc",
        SortOrder::Descending => "desc",
    }
}

fn escape(value: &str) -> String {
    value.replace('|', "\\|").replace(['\n', '\r'], " ")
}
