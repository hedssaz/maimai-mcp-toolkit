use maimai_app::rankings::{
    B50MemberRank, B50Report, B50Row, B50Sort, OutputMode, RankingLaunch, SortOrder,
};
use maimai_storage::CachedB50Entry;
use time::UtcOffset;

use super::value::{difficulty, display_time, fit_label, fit_ratio};

pub fn b50_started(launch: &RankingLaunch, offset: UtcOffset) -> String {
    format!(
        "群 {} B50 缓存不存在、已过期或被要求刷新，已启动后台刷新任务。\n任务状态: {}，启动时间: {}，原因: {}\n这次不会阻塞等待完整群查询，避免超过 AstrBot 30 秒 MCP 超时。\n稍后调用 group_b50_job_status 读取进度；完成后可直接读取榜单。",
        launch.cache.group_id.as_str(),
        launch.job.status.as_str(),
        display_time(Some(launch.job.started_at), offset, "未知"),
        super::value::reason(launch.reason),
    )
}

pub fn b50_member_started(
    launch: &RankingLaunch,
    qq: &maimai_core::QqId,
    offset: UtcOffset,
) -> String {
    format!(
        "群 {} 的 QQ {} 排名需要完整群榜缓存，当前已启动后台刷新任务。\n任务状态: {}，启动时间: {}，原因: {}\n稍后再次调用 group_b50_member_rank 读取该 QQ 的排名；也可调用 group_b50_job_status 查看进度。",
        launch.cache.group_id.as_str(),
        qq,
        launch.job.status.as_str(),
        display_time(Some(launch.job.started_at), offset, "未知"),
        super::value::reason(launch.reason),
    )
}

pub fn b50_rank_at_started(
    launch: &RankingLaunch,
    rank: usize,
    order: SortOrder,
    offset: UtcOffset,
) -> String {
    format!(
        "群 {} 的 rating {}第 {} 名需要完整群榜缓存，当前已启动后台刷新任务。\n任务状态: {}，启动时间: {}，原因: {}\n稍后再次调用 group_b50_rank_at 读取该名次；也可调用 group_b50_job_status 查看进度。",
        launch.cache.group_id.as_str(),
        order_label(order),
        rank,
        launch.job.status.as_str(),
        display_time(Some(launch.job.started_at), offset, "未知"),
        super::value::reason(launch.reason),
    )
}

pub fn b50_report(report: &B50Report, offset: UtcOffset) -> String {
    let options = report.options;
    let status = if report.cache.fresh {
        "本次使用一天内缓存，未重新拉取"
    } else {
        "本次已重新拉取并刷新缓存"
    };
    let mut lines = vec![
        format!(
            "群 {} B50 榜单（{} {}）",
            report.cache.group_id.as_str(),
            match options.sort {
                B50Sort::Rating => "rating",
                B50Sort::FitIndex => "虚高指数",
            },
            order_label(options.order),
        ),
        format!(
            "缓存状态: {}，缓存生成时间: {}",
            status,
            display_time(report.cache.fetched_at, offset, "未知"),
        ),
        format!(
            "筛选: rating {}，虚高指数 {}，输出: {}，匹配: {} 条，本次展示: {} 条",
            rating_filter(options.rating_min, options.rating_max),
            fit_filter(options.fit_min, options.fit_max),
            window_label(options.window),
            report.matched_count,
            report.rows.len(),
        ),
        format!(
            "缓存内容: 仅保存可查询且 rating>0 的完整 B50 歌曲明细；群成员: {}，已缓存: {}，跳过: {}",
            report.cache.member_count.unwrap_or_default(),
            report.cache.success_count.unwrap_or_default(),
            report.cache.skipped_count.unwrap_or_default(),
        ),
        String::new(),
        table(&report.rows),
    ];
    if options.output == OutputMode::Detail {
        for row in &report.rows {
            lines.push(String::new());
            lines.push(format!(
                "## {}. {} ({})",
                row.rank, row.entry.member.display_name, row.entry.member.qq
            ));
            lines.push(detail(&row.entry));
        }
    }
    lines.join("\n")
}

pub fn b50_member(rank: &B50MemberRank, output: OutputMode, offset: UtcOffset) -> String {
    let Some(target) = &rank.target else {
        return format!(
            "群 {} QQ {} 没有可输出的排名。\n缓存状态: 本次使用一天内缓存，未重新拉取，缓存生成时间: {}\n当前可排名成员: {} / 群成员 {}，跳过: {}\n可能原因: 水鱼按 QQ 查不到、对方隐私/未开放第三方查询，或 rating=0；这些成员按规则不输出也不缓存。",
            rank.cache.group_id.as_str(),
            rank.qq,
            display_time(rank.cache.fetched_at, offset, "未知"),
            rank.cache.success_count.unwrap_or_default(),
            rank.cache.member_count.unwrap_or_default(),
            rank.cache.skipped_count.unwrap_or_default(),
        );
    };
    let mut lines = vec![
        format!(
            "群 {} QQ {} 排名信息",
            rank.cache.group_id.as_str(),
            rank.qq
        ),
        format!(
            "缓存状态: 本次使用一天内缓存，未重新拉取，缓存生成时间: {}",
            display_time(rank.cache.fetched_at, offset, "未知")
        ),
        format!("成员: {}", identity(target)),
        format!("Rating: {}", target.player.rating.unwrap_or_default()),
        format!("虚高指数: {}", fit_cell(target)),
        format!(
            "倒序排名: {} / {}（前面 {} 人，后面 {} 人）",
            rank.rank_desc.unwrap_or_default(),
            rank.total_ranked,
            rank.rank_desc.unwrap_or(1).saturating_sub(1),
            rank.total_ranked
                .saturating_sub(rank.rank_desc.unwrap_or_default())
        ),
        format!(
            "正序排名: {} / {}",
            rank.rank_asc.unwrap_or_default(),
            rank.total_ranked
        ),
        String::new(),
        "附近排名（排名正序）:".to_owned(),
        context_table(&rank.context, &rank.qq),
    ];
    if output == OutputMode::Detail {
        lines.extend([String::new(), "完整 B50:".to_owned(), detail(target)]);
    }
    lines.join("\n")
}

pub fn b50_rank_at(
    row: &B50Row,
    member: &B50MemberRank,
    order: SortOrder,
    output: OutputMode,
    offset: UtcOffset,
) -> String {
    let mut text = format!(
        "群 {} rating {}第 {} 名\n缓存状态: 本次使用一天内缓存，未重新拉取，缓存生成时间: {}\n成员: {}\nQQ: {}\nRating: {}\n虚高指数: {}\n全群倒序排名: {} / {}\n全群正序排名: {} / {}",
        member.cache.group_id.as_str(),
        order_label(order),
        row.rank,
        display_time(member.cache.fetched_at, offset, "未知"),
        identity(&row.entry),
        row.entry.member.qq,
        row.entry.player.rating.unwrap_or_default(),
        fit_cell(&row.entry),
        member.rank_desc.unwrap_or_default(),
        member.total_ranked,
        member.rank_asc.unwrap_or_default(),
        member.total_ranked,
    );
    if output == OutputMode::Detail {
        text.push_str("\n\n完整 B50:\n");
        text.push_str(&detail(&row.entry));
    }
    text
}

pub fn b50_rank_at_missing(group: &str, rank: usize, order: SortOrder) -> String {
    format!(
        "群 {group} rating {}第 {rank} 名不存在。",
        order_label(order)
    )
}

fn table(rows: &[B50Row]) -> String {
    let mut lines = vec![
        "| 排名 | QQ | QQ昵称 | QQ群昵称 | 水鱼昵称 | Rating | 虚高指数 | 状态 |".to_owned(),
        "| --- | --- | --- | --- | --- | ---: | --- | --- |".to_owned(),
    ];
    for row in rows {
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} | OK |",
            row.rank,
            row.entry.member.qq,
            escape(row.entry.member.nickname.as_deref().unwrap_or_default()),
            escape(&row.entry.member.display_name),
            escape(
                row.entry
                    .player
                    .nickname
                    .as_deref()
                    .or(row.entry.member.waterfish_nickname.as_deref())
                    .unwrap_or_default()
            ),
            row.entry.player.rating.unwrap_or_default(),
            fit_cell(&row.entry),
        ));
    }
    lines.join("\n")
}

fn context_table(rows: &[B50Row], target: &maimai_core::QqId) -> String {
    let mut lines = vec![
        "| 排名 | QQ | QQ昵称 | QQ群昵称 | 水鱼昵称 | Rating | 虚高指数 | 标记 |".to_owned(),
        "| --- | --- | --- | --- | --- | ---: | --- | --- |".to_owned(),
    ];
    for row in rows {
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            row.rank,
            row.entry.member.qq,
            escape(row.entry.member.nickname.as_deref().unwrap_or_default()),
            escape(&row.entry.member.display_name),
            escape(row.entry.player.nickname.as_deref().unwrap_or_default()),
            row.entry.player.rating.unwrap_or_default(),
            fit_cell(&row.entry),
            if &row.entry.member.qq == target {
                "目标"
            } else {
                ""
            },
        ));
    }
    lines.join("\n")
}

fn detail(entry: &CachedB50Entry) -> String {
    let mut lines = vec![format!(
        "玩家: {} / Rating {}（B35 {} + B15 {}）",
        entry.player.nickname.as_deref().unwrap_or("?"),
        entry.rating_breakdown.total,
        entry.rating_breakdown.b35,
        entry.rating_breakdown.b15,
    )];
    for section in [
        maimai_storage::B50Section::B35,
        maimai_storage::B50Section::B15,
    ] {
        lines.push(format!(
            "{}:",
            if section == maimai_storage::B50Section::B35 {
                "B35"
            } else {
                "B15"
            }
        ));
        for chart in entry.charts.iter().filter(|chart| chart.section == section) {
            lines.push(format!(
                "- {} [{} {}] {}% / {} ra",
                chart.chart.title,
                difficulty(chart.chart.key.difficulty()),
                chart.chart.level,
                chart
                    .chart
                    .achievements
                    .map(|value| value.ten_thousandths() as f64 / 10_000.0)
                    .unwrap_or_default(),
                chart.chart.rating.unwrap_or_default(),
            ));
        }
    }
    lines.join("\n")
}

fn identity(entry: &CachedB50Entry) -> String {
    format!(
        "QQ昵称 {} / QQ群昵称 {} / 水鱼昵称 {}",
        entry.member.nickname.as_deref().unwrap_or("未知QQ昵称"),
        entry.member.display_name,
        entry.player.nickname.as_deref().unwrap_or("未知水鱼昵称"),
    )
}

fn fit_cell(entry: &CachedB50Entry) -> String {
    fit_ratio(entry).map_or_else(
        || "数据不足".to_owned(),
        |ratio| {
            format!(
                "{ratio:+.2}% {:+} ra {}",
                entry.fit_index.b50.virtual_rating.unwrap_or_default(),
                fit_label(ratio)
            )
        },
    )
}

fn rating_filter(min: Option<u32>, max: Option<u32>) -> String {
    match (min, max) {
        (None, None) => "无".to_owned(),
        (Some(min), Some(max)) => format!("{min} 到 {max}"),
        (Some(min), None) => format!("{min} 及以上"),
        (None, Some(max)) => format!("{max} 及以下"),
    }
}

fn fit_filter(
    min: Option<maimai_app::scores::ExactRatio>,
    max: Option<maimai_app::scores::ExactRatio>,
) -> String {
    match (min.map(ratio), max.map(ratio)) {
        (None, None) => "无".to_owned(),
        (Some(min), Some(max)) => format!("{min:+.2}% 到 {max:+.2}%"),
        (Some(min), None) => format!("{min:+.2}% 及以上"),
        (None, Some(max)) => format!("{max:+.2}% 及以下"),
    }
}

fn ratio(value: maimai_app::scores::ExactRatio) -> f64 {
    value.numerator() as f64 / value.denominator() as f64
}

fn window_label(window: maimai_app::rankings::RankWindow) -> String {
    if let Some((start, end)) = window.start.zip(window.end) {
        format!("第 {start}-{end} 名")
    } else if let Some(limit) = window.limit {
        format!("前 {limit} 人")
    } else {
        "无上限".to_owned()
    }
}

fn order_label(order: SortOrder) -> &'static str {
    match order {
        SortOrder::Ascending => "正序",
        SortOrder::Descending => "倒序",
    }
}

fn escape(value: &str) -> String {
    value.replace('|', "\\|").replace(['\n', '\r'], " ")
}
