use std::sync::Arc;

use maimai_core::{GroupId, QqId};
use maimai_providers::NapCatClient;
use maimai_storage::{CachedB50Entry, RankingNamespace};
use time::OffsetDateTime;

use super::super::{
    B50MemberRank, B50Report, B50ReportOptions, B50Row, B50Sort, ContextSize, RankingError,
    RankingResponse, RankingService, RefreshOptions, SortOrder,
};

impl RankingService {
    pub async fn b50_report(
        &self,
        group_id: GroupId,
        options: B50ReportOptions,
        force: bool,
        refresh: RefreshOptions,
        now: OffsetDateTime,
    ) -> Result<RankingResponse<B50Report>, RankingError> {
        self.b50_report_with_client(
            group_id,
            options,
            force,
            refresh,
            now,
            Arc::clone(&self.napcat),
        )
        .await
    }

    pub async fn b50_report_with_client(
        &self,
        group_id: GroupId,
        options: B50ReportOptions,
        force: bool,
        refresh: RefreshOptions,
        now: OffsetDateTime,
        client: Arc<NapCatClient>,
    ) -> Result<RankingResponse<B50Report>, RankingError> {
        validate_b50_options(options)?;
        if let Some(launch) = self
            .ensure_cache_with_client(
                RankingNamespace::B50,
                group_id.clone(),
                force,
                refresh,
                now,
                client,
            )
            .await?
        {
            return Ok(RankingResponse::Started(Box::new(launch)));
        }
        Ok(RankingResponse::Ready(
            self.b50_cached_report(&group_id, options, now).await?,
        ))
    }

    pub async fn b50_cached_report(
        &self,
        group_id: &GroupId,
        options: B50ReportOptions,
        now: OffsetDateTime,
    ) -> Result<B50Report, RankingError> {
        validate_b50_options(options)?;
        let entries = self
            .b50_cache(group_id)
            .await?
            .ok_or(RankingError::NotFound)?;
        let mut rows = entries
            .iter()
            .filter(|entry| rating_matches(entry, options.rating_min, options.rating_max))
            .filter(|entry| fit_matches(entry, options.fit_min, options.fit_max))
            .cloned()
            .collect::<Vec<_>>();
        sort_b50(&mut rows, options.sort, options.order);
        let matched_count = rows.len();
        let rows = apply_window(rows, options.window)?;
        Ok(B50Report {
            cache: self
                .cache_status(RankingNamespace::B50, group_id, now)
                .await?,
            options,
            matched_count,
            all_entries: entries,
            rows,
        })
    }

    pub async fn b50_member_rank_cached(
        &self,
        group_id: &GroupId,
        qq: QqId,
        context_size: ContextSize,
        now: OffsetDateTime,
    ) -> Result<B50MemberRank, RankingError> {
        let entries = self
            .b50_cache(group_id)
            .await?
            .ok_or(RankingError::NotFound)?;
        let mut desc = entries.clone();
        let mut asc = entries;
        sort_b50(&mut desc, B50Sort::Rating, SortOrder::Descending);
        sort_b50(&mut asc, B50Sort::Rating, SortOrder::Ascending);
        let desc_index = desc.iter().position(|entry| entry.member.qq == qq);
        let asc_index = asc.iter().position(|entry| entry.member.qq == qq);
        let total = desc.len();
        let context = desc_index.map_or_else(Vec::new, |index| {
            let start = index.saturating_sub(context_size.get());
            let end = total.min(index + context_size.get() + 1);
            desc[start..end]
                .iter()
                .cloned()
                .enumerate()
                .map(|(offset, entry)| B50Row {
                    rank: start + offset + 1,
                    entry,
                })
                .collect()
        });
        Ok(B50MemberRank {
            cache: self
                .cache_status(RankingNamespace::B50, group_id, now)
                .await?,
            qq,
            found: desc_index.is_some(),
            target: desc_index.map(|index| desc[index].clone()),
            rank_desc: desc_index.map(|index| index + 1),
            rank_asc: asc_index.map(|index| index + 1),
            total_ranked: total,
            context,
        })
    }
}

fn validate_b50_options(options: B50ReportOptions) -> Result<(), RankingError> {
    options.window.validate()?;
    if options
        .rating_min
        .zip(options.rating_max)
        .is_some_and(|(min, max)| min > max)
        || options
            .fit_min
            .zip(options.fit_max)
            .is_some_and(|(min, max)| min > max)
    {
        return Err(RankingError::InvalidInput { field: "range" });
    }
    Ok(())
}

fn rating_matches(entry: &CachedB50Entry, min: Option<u32>, max: Option<u32>) -> bool {
    entry.player.rating.is_some_and(|rating| {
        min.is_none_or(|min| rating >= min) && max.is_none_or(|max| rating <= max)
    })
}

fn fit_matches(
    entry: &CachedB50Entry,
    min: Option<crate::scores::ExactRatio>,
    max: Option<crate::scores::ExactRatio>,
) -> bool {
    if min.is_none() && max.is_none() {
        return true;
    }
    cached_ratio(entry)
        .is_some_and(|fit| min.is_none_or(|min| fit >= min) && max.is_none_or(|max| fit <= max))
}

fn sort_b50(entries: &mut [CachedB50Entry], sort: B50Sort, order: SortOrder) {
    entries.sort_by(|left, right| {
        let primary = match sort {
            B50Sort::Rating => left.player.rating.cmp(&right.player.rating),
            B50Sort::FitIndex => match (cached_ratio(left), cached_ratio(right)) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            },
        };
        let ordering = primary
            .then_with(|| left.player.rating.cmp(&right.player.rating))
            .then_with(|| left.member.display_name.cmp(&right.member.display_name))
            .then_with(|| left.member.qq.cmp(&right.member.qq));
        match order {
            SortOrder::Ascending => ordering,
            SortOrder::Descending => ordering.reverse(),
        }
    });
}

fn cached_ratio(entry: &CachedB50Entry) -> Option<crate::scores::ExactRatio> {
    let ratio = entry.fit_index.b50.virtual_ratio_percent?;
    crate::scores::ExactRatio::new(ratio.numerator, ratio.denominator)
}

fn apply_window(
    entries: Vec<CachedB50Entry>,
    window: super::super::RankWindow,
) -> Result<Vec<B50Row>, RankingError> {
    let (start, end) = if let Some((start, end)) = window.start.zip(window.end) {
        (start - 1, end.min(entries.len()))
    } else {
        (0, window.limit.unwrap_or(entries.len()).min(entries.len()))
    };
    if start >= entries.len() {
        return Ok(Vec::new());
    }
    Ok(entries[start..end]
        .iter()
        .cloned()
        .enumerate()
        .map(|(offset, entry)| B50Row {
            rank: start + offset + 1,
            entry,
        })
        .collect())
}
