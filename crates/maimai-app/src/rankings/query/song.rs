use std::{collections::BTreeMap, sync::Arc};

use maimai_catalog::{CatalogQuery, SongIdFilter};
use maimai_core::{
    ChartGeneration, Difficulty, GroupId, QqId, SongIdNamespace, SongIdValue, SourceSongId,
};
use maimai_providers::NapCatClient;
use maimai_storage::{CachedChart, RankingMember, RankingNamespace};
use time::OffsetDateTime;

use super::super::{
    ContextSize, RankingError, RankingResponse, RankingService, RefreshOptions, SongMemberRank,
    SongReport, SongReportOptions, SongRow, SongSort, SongTarget, SortOrder,
};

impl RankingService {
    pub fn resolve_song(
        &self,
        query: Option<&str>,
        music_id: Option<u32>,
        difficulty: Option<Difficulty>,
        deluxe: Option<bool>,
        limit: usize,
    ) -> Result<SongTarget, RankingError> {
        let snapshot = self.catalog.snapshot();
        let hit = if let Some(music_id) = music_id {
            let exact = SourceSongId::numeric(SongIdNamespace::DivingFish, music_id);
            snapshot
                .query(&CatalogQuery {
                    id: Some(SongIdFilter::Exact(exact)),
                    limit: Some(1),
                    ..CatalogQuery::default()
                })
                .map_err(|_| RankingError::Catalog)?
                .into_iter()
                .next()
                .or_else(|| {
                    snapshot
                        .query(&CatalogQuery {
                            id: Some(SongIdFilter::AnySource(SongIdValue::Numeric(music_id))),
                            limit: Some(1),
                            ..CatalogQuery::default()
                        })
                        .ok()
                        .and_then(|hits| hits.into_iter().next())
                })
        } else if let Some(query) = query {
            snapshot.search_text(query, limit.max(1)).into_iter().next()
        } else {
            return Err(RankingError::InvalidInput {
                field: "songQuery or musicId",
            });
        }
        .ok_or(RankingError::NotFound)?;
        Ok(SongTarget {
            title: hit.music.title.clone(),
            ids: hit.music.source_ids.clone(),
            difficulty,
            deluxe,
        })
    }

    pub async fn song_report(
        &self,
        group_id: GroupId,
        options: Option<SongReportOptions>,
        force: bool,
        refresh: RefreshOptions,
        now: OffsetDateTime,
    ) -> Result<RankingResponse<Option<SongReport>>, RankingError> {
        self.song_report_with_client(
            group_id,
            options,
            force,
            refresh,
            now,
            Arc::clone(&self.napcat),
        )
        .await
    }

    pub async fn song_report_with_client(
        &self,
        group_id: GroupId,
        options: Option<SongReportOptions>,
        force: bool,
        refresh: RefreshOptions,
        now: OffsetDateTime,
        client: Arc<NapCatClient>,
    ) -> Result<RankingResponse<Option<SongReport>>, RankingError> {
        if let Some(options) = &options {
            validate_song_options(options)?;
        }
        if let Some(launch) = self
            .ensure_cache_with_client(
                RankingNamespace::SongScore,
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
        let report = match options {
            Some(options) => Some(self.song_cached_report(&group_id, options, now).await?),
            None => None,
        };
        Ok(RankingResponse::Ready(report))
    }

    pub async fn song_cached_report(
        &self,
        group_id: &GroupId,
        mut options: SongReportOptions,
        now: OffsetDateTime,
    ) -> Result<SongReport, RankingError> {
        validate_song_options(&options)?;
        let (members, records) = self
            .song_cache(group_id)
            .await?
            .ok_or(RankingError::NotFound)?;
        let member_map = members
            .into_iter()
            .map(|member| (member.qq.clone(), member))
            .collect::<BTreeMap<_, _>>();
        options.target = choose_difficulty(options.target, &records);
        let mut rows = collect_rows(&member_map, &records, &options);
        sort_song(&mut rows, options.sort, options.order);
        let matched_count = rows.len();
        let rows = window_song(rows, options.window);
        Ok(SongReport {
            cache: self
                .cache_status(RankingNamespace::SongScore, group_id, now)
                .await?,
            target: options.target,
            sort: options.sort,
            order: options.order,
            matched_count,
            rows,
        })
    }

    pub async fn song_member_rank_cached(
        &self,
        group_id: &GroupId,
        qq: QqId,
        target: SongTarget,
        context_size: ContextSize,
        now: OffsetDateTime,
    ) -> Result<SongMemberRank, RankingError> {
        let options = SongReportOptions {
            target,
            sort: SongSort::Achievements,
            order: SortOrder::Descending,
            achievements_min: None,
            achievements_max: None,
            window: Default::default(),
        };
        let mut report = self.song_cached_report(group_id, options, now).await?;
        let mut desc = report.rows.clone();
        let mut asc = report.rows.clone();
        sort_song(&mut desc, SongSort::Achievements, SortOrder::Descending);
        sort_song(&mut asc, SongSort::Achievements, SortOrder::Ascending);
        rerank(&mut desc);
        rerank(&mut asc);
        let desc_index = desc.iter().position(|row| row.qq == qq);
        let asc_index = asc.iter().position(|row| row.qq == qq);
        let total = desc.len();
        let context = context_rows(&desc, desc_index, context_size);
        let reverse_context = context_rows(&asc, asc_index, context_size);
        let row = desc_index.map(|index| desc[index].clone());
        report.rows.clear();
        Ok(SongMemberRank {
            cache: report.cache,
            qq,
            target: report.target,
            found: row.is_some(),
            row,
            rank_desc: desc_index.map(|index| index + 1),
            rank_asc: asc_index.map(|index| index + 1),
            total_ranked: total,
            context,
            reverse_context,
        })
    }
}

fn validate_song_options(options: &SongReportOptions) -> Result<(), RankingError> {
    options.window.validate()?;
    if options
        .achievements_min
        .zip(options.achievements_max)
        .is_some_and(|(min, max)| min > max)
    {
        return Err(RankingError::InvalidInput {
            field: "achievements range",
        });
    }
    Ok(())
}

fn choose_difficulty(mut target: SongTarget, records: &[(QqId, CachedChart)]) -> SongTarget {
    let available = records
        .iter()
        .filter(|(_, chart)| song_matches(chart, &target))
        .map(|(_, chart)| chart.key.difficulty())
        .collect::<std::collections::BTreeSet<_>>();
    match target.difficulty {
        None => target.difficulty = available.iter().next_back().copied(),
        Some(requested) if !available.contains(&requested) && available.len() == 1 => {
            target.difficulty = available.iter().next().copied();
        }
        _ => {}
    }
    target
}

fn collect_rows(
    members: &BTreeMap<QqId, RankingMember>,
    records: &[(QqId, CachedChart)],
    options: &SongReportOptions,
) -> Vec<SongRow> {
    let mut best = BTreeMap::<QqId, CachedChart>::new();
    for (qq, chart) in records {
        if !song_matches(chart, &options.target)
            || options
                .target
                .difficulty
                .is_some_and(|difficulty| chart.key.difficulty() != difficulty)
            || options
                .achievements_min
                .is_some_and(|min| chart.achievements.is_none_or(|value| value < min))
            || options
                .achievements_max
                .is_some_and(|max| chart.achievements.is_none_or(|value| value > max))
        {
            continue;
        }
        match best.entry(qq.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(chart.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if (chart.rating, chart.achievements)
                    > (entry.get().rating, entry.get().achievements)
                {
                    entry.insert(chart.clone());
                }
            }
        }
    }
    best.into_iter()
        .filter_map(|(qq, record)| {
            members.get(&qq).cloned().map(|member| SongRow {
                rank: 0,
                qq,
                member,
                record,
            })
        })
        .collect()
}

fn song_matches(chart: &CachedChart, target: &SongTarget) -> bool {
    target.ids.contains(chart.key.song())
        && target.deluxe.is_none_or(|deluxe| {
            matches!(chart.key.generation(), ChartGeneration::Deluxe) == deluxe
        })
}

fn sort_song(rows: &mut [SongRow], sort: SongSort, order: SortOrder) {
    rows.sort_by(|left, right| {
        let primary = match sort {
            SongSort::Achievements => left.record.achievements.cmp(&right.record.achievements),
            SongSort::Rating => left.record.rating.cmp(&right.record.rating),
            SongSort::DxScore => left.record.dx_score.cmp(&right.record.dx_score),
        };
        let ordering = primary
            .then_with(|| left.record.achievements.cmp(&right.record.achievements))
            .then_with(|| left.qq.cmp(&right.qq));
        match order {
            SortOrder::Ascending => ordering,
            SortOrder::Descending => ordering.reverse(),
        }
    });
}

fn window_song(mut rows: Vec<SongRow>, window: super::super::RankWindow) -> Vec<SongRow> {
    rerank(&mut rows);
    let (start, end) = if let Some((start, end)) = window.start.zip(window.end) {
        (start - 1, end.min(rows.len()))
    } else {
        (0, window.limit.unwrap_or(rows.len()).min(rows.len()))
    };
    if start >= rows.len() {
        Vec::new()
    } else {
        rows[start..end].to_vec()
    }
}

fn rerank(rows: &mut [SongRow]) {
    for (index, row) in rows.iter_mut().enumerate() {
        row.rank = index + 1;
    }
}

fn context_rows(rows: &[SongRow], index: Option<usize>, size: ContextSize) -> Vec<SongRow> {
    index.map_or_else(Vec::new, |index| {
        let start = index.saturating_sub(size.get());
        let end = rows.len().min(index + size.get() + 1);
        rows[start..end].to_vec()
    })
}
