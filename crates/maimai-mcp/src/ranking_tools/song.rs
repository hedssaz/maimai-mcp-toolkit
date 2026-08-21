use maimai_app::rankings::{RankingResponse, SongReport};
use maimai_core::{SongIdNamespace, SongIdValue};
use maimai_storage::RankingNamespace;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use super::{
    RankingDispatcher, convert,
    dto::{SongMemberArgs, SongReportArgs},
    error::RankingToolError,
    format,
};
use crate::{DispatchError, ToolOutput};

impl RankingDispatcher {
    pub(super) async fn song_report(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: SongReportArgs = super::deserialize(arguments)?;
        let group = convert::group_id(args.group_id.clone())?;
        let music_id = convert::music_id(args.music_id.clone())?;
        let search_limit = search_limit(args.search_limit)?;
        let has_query = args
            .song_query
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        let options = if has_query || music_id.is_some() {
            let target = self
                .service
                .resolve_song(
                    args.song_query.as_deref(),
                    music_id,
                    convert::difficulty(args.level_index)?,
                    convert::deluxe(args.song_type.clone())?,
                    search_limit,
                )
                .map_err(RankingToolError::from)?;
            Some(convert::song_options(
                target,
                args.sort_by.clone(),
                args.sort_order.clone(),
                args.achievements_min.clone(),
                args.achievements_max.clone(),
                convert::window(args.output_limit, args.start_rank, args.end_rank)?,
            )?)
        } else {
            None
        };
        let refresh_args = args.refresh();
        let response = self
            .service
            .song_report_with_client(
                group,
                options,
                args.force_refresh.unwrap_or(false),
                convert::refresh(&refresh_args)?,
                OffsetDateTime::now_utc(),
                convert::napcat_client(self.service.napcat(), &refresh_args)?,
            )
            .await
            .map_err(RankingToolError::from)?;
        match response {
            RankingResponse::Started(launch) => {
                let text = format::song_started(&launch, music_id, self.display_offset);
                super::output(
                    text.clone(),
                    json!({
                        "groupId": launch.cache.group_id.as_str(), "musicId": music_id,
                        "levelIndex": args.level_index, "songType": args.song_type,
                        "sortBy": args.sort_by.as_deref().unwrap_or("achievements"),
                        "sortOrder": args.sort_order.as_deref().unwrap_or("desc"),
                        "outputLimit": args.output_limit, "startRank": args.start_rank, "endRank": args.end_rank,
                        "cache": format::cache_value(&launch.cache), "job": format::job_value(&launch.job),
                        "cacheRefreshReason": format::reason(launch.reason), "text": text, "data": Value::Null,
                    }),
                )
            }
            RankingResponse::Ready(None) => {
                let status = self
                    .service
                    .cache_status(
                        RankingNamespace::SongScore,
                        &convert::group_id(args.group_id)?,
                        OffsetDateTime::now_utc(),
                    )
                    .await
                    .map_err(RankingToolError::from)?;
                let text = format::song_cache_ready(&status, self.display_offset);
                super::output(
                    text.clone(),
                    json!({
                        "groupId": status.group_id.as_str(), "musicId": Value::Null,
                        "levelIndex": args.level_index, "songType": args.song_type,
                        "sortBy": args.sort_by.as_deref().unwrap_or("achievements"),
                        "sortOrder": args.sort_order.as_deref().unwrap_or("desc"),
                        "outputLimit": args.output_limit, "startRank": args.start_rank, "endRank": args.end_rank,
                        "cache": format::cache_value(&status), "cacheRefreshReason": "hit",
                        "text": text, "data": format::cache_value(&status),
                    }),
                )
            }
            RankingResponse::Ready(Some(report)) => {
                song_report_output(&report, self.display_offset)
            }
        }
    }

    pub(super) async fn song_member_rank(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: SongMemberArgs = super::deserialize(arguments)?;
        let (qq, group) = self
            .service
            .resolve_member(
                convert::optional_qq(args.qq.clone())?,
                args.target.as_deref(),
                convert::optional_group_id(args.group_id.clone())?,
            )
            .await
            .map_err(RankingToolError::from)?;
        let music_id = convert::music_id(args.music_id.clone())?;
        let search_limit = search_limit(args.search_limit)?;
        let target = self
            .service
            .resolve_song(
                args.song_query.as_deref(),
                music_id,
                convert::difficulty(args.level_index)?,
                convert::deluxe(args.song_type.clone())?,
                search_limit,
            )
            .map_err(RankingToolError::from)?;
        let refresh_args = args.refresh();
        let now = OffsetDateTime::now_utc();
        if let Some(launch) = self
            .service
            .ensure_cache_with_client(
                RankingNamespace::SongScore,
                group.clone(),
                args.force_refresh.unwrap_or(false),
                convert::refresh(&refresh_args)?,
                now,
                convert::napcat_client(self.service.napcat(), &refresh_args)?,
            )
            .await
            .map_err(RankingToolError::from)?
        {
            let text = format::song_member_started(&launch, &qq, music_id, self.display_offset);
            return super::output(
                text.clone(),
                json!({
                    "groupId": group.as_str(), "qq": qq.as_str(), "musicId": music_id,
                    "levelIndex": args.level_index, "songType": args.song_type,
                    "contextSize": args.context_size.unwrap_or(3),
                    "cache": format::cache_value(&launch.cache), "job": format::job_value(&launch.job),
                    "cacheRefreshReason": format::reason(launch.reason), "text": text, "data": Value::Null,
                }),
            );
        }
        let context = convert::context(args.context_size)?;
        let result = self
            .service
            .song_member_rank_cached(&group, qq.clone(), target, context, now)
            .await
            .map_err(RankingToolError::from)?;
        let text = if result.found {
            format::song_member(&result, self.display_offset)
        } else {
            format::song_member_missing(&result, self.display_offset)
        };
        let row = result
            .row
            .as_ref()
            .map(|row| song_row_value(row, group.as_str()));
        let context_rows = result
            .context
            .iter()
            .map(|row| song_row_value(row, group.as_str()))
            .collect::<Vec<_>>();
        let reverse_rows = result
            .reverse_context
            .iter()
            .map(|row| song_row_value(row, group.as_str()))
            .collect::<Vec<_>>();
        super::output(
            text.clone(),
            json!({
                "groupId": group.as_str(), "qq": qq.as_str(), "musicId": diving_fish_id(&result.target),
                "found": result.found, "rank": result.rank_desc, "reverseRank": result.rank_asc,
                "rankInfo": {"rankDesc": result.rank_desc, "rankAsc": result.rank_asc,
                    "totalRanked": result.total_ranked,
                    "higherCount": result.rank_desc.map(|rank| rank.saturating_sub(1)),
                    "lowerCount": result.rank_desc.map(|rank| result.total_ranked.saturating_sub(rank))},
                "totalRanked": result.total_ranked, "target": row, "context": context_rows,
                "reverseContext": reverse_rows, "cache": format::cache_value(&result.cache),
                "text": text, "data": {"target": row, "rank": result.rank_desc,
                    "rankInfo": {"rankDesc": result.rank_desc, "rankAsc": result.rank_asc,
                        "totalRanked": result.total_ranked}},
            }),
        )
    }
}

fn song_report_output(
    report: &SongReport,
    offset: time::UtcOffset,
) -> Result<ToolOutput, DispatchError> {
    let text = format::song_report(report, offset);
    super::output(
        text.clone(),
        json!({
            "groupId": report.cache.group_id.as_str(), "musicId": diving_fish_id(&report.target),
            "levelIndex": report.target.difficulty.and_then(format::difficulty_index),
            "songType": report.target.deluxe.map(|value| if value { "DX" } else { "SD" }),
            "sortBy": song_sort_name(report.sort), "sortOrder": order_name(report.order),
            "matchedCount": report.matched_count,
        "rows": report.rows.iter().map(|row| song_row_value(row, report.cache.group_id.as_str())).collect::<Vec<_>>(),
            "cache": format::cache_value(&report.cache), "cacheRefreshReason": "hit",
            "text": text, "data": format::cache_value(&report.cache),
        }),
    )
}

fn song_row_value(row: &maimai_app::rankings::SongRow, group_id: &str) -> Value {
    json!({
        "_rank": row.rank, "userId": row.qq.as_str(), "displayName": row.member.display_name,
        "nickname": row.member.nickname, "card": row.member.card,
        "identity": format::identity_value(&row.member, group_id),
        "playerNickname": row.member.waterfish_nickname,
        "record": format::chart_value(&row.record),
    })
}

fn diving_fish_id(target: &maimai_app::rankings::SongTarget) -> Option<u32> {
    target
        .ids
        .iter()
        .find_map(|id| match (id.namespace(), id.value()) {
            (SongIdNamespace::DivingFish, SongIdValue::Numeric(value)) => Some(*value),
            _ => None,
        })
}

const fn song_sort_name(value: maimai_app::rankings::SongSort) -> &'static str {
    match value {
        maimai_app::rankings::SongSort::Achievements => "achievements",
        maimai_app::rankings::SongSort::Rating => "ra",
        maimai_app::rankings::SongSort::DxScore => "dxScore",
    }
}

const fn order_name(value: maimai_app::rankings::SortOrder) -> &'static str {
    match value {
        maimai_app::rankings::SortOrder::Ascending => "asc",
        maimai_app::rankings::SortOrder::Descending => "desc",
    }
}

fn search_limit(value: Option<usize>) -> Result<usize, RankingToolError> {
    let value = value.unwrap_or(5);
    if (1..=20).contains(&value) {
        Ok(value)
    } else {
        Err(RankingToolError::invalid("searchLimit 必须在 1 到 20。"))
    }
}
