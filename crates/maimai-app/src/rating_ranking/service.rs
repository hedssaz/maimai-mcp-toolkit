use std::sync::Arc;

use caseless::default_case_fold_str;
use maimai_core::PlayerSelector;
use maimai_providers::{DivingFishRatingEntry, DivingFishScoreClient};
use maimai_render::{RatingRankingDocument, RatingRankingRenderer};
use time::{OffsetDateTime, UtcOffset};
use tokio::sync::Semaphore;

use crate::image_output::ImageOutputStore;

use super::{
    RatingRankingError, RatingRankingImage, RatingRankingRequest, RatingRankingTarget,
    RatingRankingUsername, model::TargetKind,
};

const PAGE_SIZE: usize = 50;
const DEFAULT_MAX_RENDER_CONCURRENCY: usize = 2;

pub struct RatingRankingService {
    diving_fish: DivingFishScoreClient,
    renderer: Arc<RatingRankingRenderer>,
    outputs: ImageOutputStore,
    render_slots: Arc<Semaphore>,
    display_offset: UtcOffset,
}

impl RatingRankingService {
    pub fn new(
        diving_fish: DivingFishScoreClient,
        renderer: RatingRankingRenderer,
        outputs: ImageOutputStore,
        display_offset: UtcOffset,
    ) -> Self {
        Self::with_max_render_concurrency(
            diving_fish,
            renderer,
            outputs,
            display_offset,
            DEFAULT_MAX_RENDER_CONCURRENCY,
        )
    }

    pub fn with_max_render_concurrency(
        diving_fish: DivingFishScoreClient,
        renderer: RatingRankingRenderer,
        outputs: ImageOutputStore,
        display_offset: UtcOffset,
        max_render_concurrency: usize,
    ) -> Self {
        Self {
            diving_fish,
            renderer: Arc::new(renderer),
            outputs,
            render_slots: Arc::new(Semaphore::new(max_render_concurrency.max(1))),
            display_offset,
        }
    }

    pub async fn render(
        &self,
        request: RatingRankingRequest,
    ) -> Result<RatingRankingImage, RatingRankingError> {
        let target = self.resolve_target(request.target).await?;
        let ranking = prepare_ranking(self.diving_fish.rating_ranking().await?)?;
        let lines = document_lines(&ranking, &target, request.now, self.display_offset)?;
        let document = RatingRankingDocument::new(lines)?;
        let permit = Arc::clone(&self.render_slots)
            .acquire_owned()
            .await
            .map_err(|_| RatingRankingError::TaskJoin)?;
        let renderer = Arc::clone(&self.renderer);
        let outputs = self.outputs.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let rendered = renderer.render(&document)?;
            let saved = outputs.save_png("rating_ranking", &rendered.bytes, request.now)?;
            Ok(RatingRankingImage {
                image_path: saved.path,
                width: rendered.width,
                height: rendered.height,
            })
        })
        .await
        .map_err(|_| RatingRankingError::TaskJoin)?
    }

    async fn resolve_target(
        &self,
        target: RatingRankingTarget,
    ) -> Result<RatingRankingTarget, RatingRankingError> {
        let Some(qq) = target.qq_value().cloned() else {
            return Ok(target);
        };
        let b50 = self
            .diving_fish
            .query_b50(PlayerSelector::Qq(qq.clone()))
            .await?;
        let username = b50
            .player
            .username
            .ok_or_else(|| RatingRankingError::MissingUsername {
                qq: qq.as_str().to_owned(),
            })?;
        let username = RatingRankingUsername::new(username.as_str().to_owned())
            .map_err(|_| RatingRankingError::InvalidProviderUsername)?;
        Ok(RatingRankingTarget::username(username))
    }
}

struct RankedUser {
    username: RatingRankingUsername,
    rating: u32,
    folded_username: String,
}

fn prepare_ranking(
    ranking: Vec<DivingFishRatingEntry>,
) -> Result<Vec<RankedUser>, RatingRankingError> {
    let mut ranking = ranking
        .into_iter()
        .map(|entry| {
            let username = RatingRankingUsername::new(entry.username.as_str().to_owned())
                .map_err(|_| RatingRankingError::InvalidProviderUsername)?;
            let folded_username = default_case_fold_str(username.as_str());
            Ok(RankedUser {
                username,
                rating: entry.rating,
                folded_username,
            })
        })
        .collect::<Result<Vec<_>, RatingRankingError>>()?;
    sort_ranking(&mut ranking);
    Ok(ranking)
}

fn sort_ranking(ranking: &mut [RankedUser]) {
    ranking.sort_by(|left, right| {
        right
            .rating
            .cmp(&left.rating)
            .then_with(|| left.folded_username.cmp(&right.folded_username))
    });
}

fn document_lines(
    ranking: &[RankedUser],
    target: &RatingRankingTarget,
    now: OffsetDateTime,
    display_offset: UtcOffset,
) -> Result<Vec<String>, RatingRankingError> {
    let timestamp = format_timestamp(now, display_offset);
    match target.kind() {
        TargetKind::Username(username) => Ok(username_lines(ranking, username, &timestamp)),
        TargetKind::Range { start, end } => Ok(range_lines(ranking, *start, *end, &timestamp)),
        TargetKind::Page(page) => Ok(page_lines(ranking, *page, &timestamp)),
        TargetKind::Qq(_) => Err(RatingRankingError::invalid(
            "QQ target 必须先解析为 username",
        )),
    }
}

fn username_lines(
    ranking: &[RankedUser],
    username: &RatingRankingUsername,
    timestamp: &str,
) -> Vec<String> {
    let needle = default_case_fold_str(username.as_str());
    ranking
        .iter()
        .position(|entry| entry.folded_username == needle)
        .map_or_else(
            || vec!["未找到该玩家".to_owned()],
            |index| {
                vec![
                    format!("截止至 {timestamp}"),
                    format!(
                        "玩家 {} 在查分器已注册用户ra排行第{}",
                        ranking[index].username.as_str(),
                        index + 1
                    ),
                ]
            },
        )
}

fn page_lines(ranking: &[RankedUser], requested_page: usize, timestamp: &str) -> Vec<String> {
    let total_pages = ranking.len().div_ceil(PAGE_SIZE).max(1);
    let page = requested_page.min(total_pages);
    let start = (page - 1) * PAGE_SIZE;
    let end = start.saturating_add(PAGE_SIZE).min(ranking.len());
    let mut lines = vec![format!("截止至 {timestamp}，查分器已注册用户ra排行：")];
    for (offset, entry) in ranking[start..end].iter().enumerate() {
        lines.push(format!(
            "No.{:02}.「{}」 {} ",
            start + offset + 1,
            entry.rating,
            entry.username.as_str()
        ));
    }
    lines.push(format!("第「{page}」页，共「{total_pages}」页"));
    lines
}

fn range_lines(
    ranking: &[RankedUser],
    start: usize,
    requested_end: usize,
    timestamp: &str,
) -> Vec<String> {
    if ranking.is_empty() || start > ranking.len() {
        return vec![
            format!("截止至 {timestamp}"),
            format!("未找到第 {start}-{requested_end} 名的公开 ranking 数据。"),
        ];
    }
    let end = requested_end.min(ranking.len());
    let mut lines = vec![
        format!("截止至 {timestamp}"),
        format!("Diving-Fish 已注册用户 ra 排行第 {start}-{end} 名"),
        String::new(),
        "排名 | username | rating".to_owned(),
        "--- | --- | ---".to_owned(),
    ];
    for (offset, entry) in ranking[(start - 1)..end].iter().enumerate() {
        lines.push(format!(
            "{} | {} | {}",
            start + offset,
            entry.username.as_str(),
            entry.rating
        ));
    }
    lines.push(String::new());
    lines.push(format!("共 {} 人", ranking.len()));
    lines
}

fn format_timestamp(value: OffsetDateTime, offset: UtcOffset) -> String {
    let value = value.to_offset(offset);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

#[cfg(test)]
mod unit_tests {
    use maimai_core::PlayerUsername;

    use super::*;

    fn entry(name: &str, rating: u32) -> Result<DivingFishRatingEntry, Box<dyn std::error::Error>> {
        Ok(DivingFishRatingEntry {
            username: PlayerUsername::new(name)?,
            rating,
        })
    }

    #[test]
    fn sorting_is_rating_desc_casefold_asc_and_stable_for_equal_keys()
    -> Result<(), Box<dyn std::error::Error>> {
        let values = vec![
            entry("zulu", 15_000)?,
            entry("Straße", 16_000)?,
            entry("STRASSE", 16_000)?,
            entry("alpha", 16_000)?,
        ];
        let values = prepare_ranking(values)?;
        assert_eq!(
            values
                .iter()
                .map(|entry| entry.username.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "Straße", "STRASSE", "zulu"]
        );
        Ok(())
    }

    #[test]
    fn exact_fifty_has_one_page_and_lookup_uses_unicode_casefold()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut provider_values = (0..48)
            .map(|index| entry(&format!("user{index:02}"), 15_000 - index))
            .collect::<Result<Vec<_>, _>>()?;
        provider_values.push(entry("Straße", 16_000)?);
        provider_values.push(entry("STRASSE", 15_999)?);
        let values = prepare_ranking(provider_values)?;
        let page = page_lines(&values, 2, "2023-01-01 00:00:00");
        assert_eq!(
            page.last().map(String::as_str),
            Some("第「1」页，共「1」页")
        );
        let lookup = username_lines(
            &values,
            &RatingRankingUsername::new("strasse")?,
            "2023-01-01 00:00:00",
        );
        assert!(lookup[1].contains("Straße"));

        let fifty_one = prepare_ranking(
            (0..51)
                .map(|index| entry(&format!("page{index:02}"), 20_000 - index))
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        let second_page = page_lines(&fifty_one, 2, "2023-01-01 00:00:00");
        assert!(second_page[1].starts_with("No.51."));
        assert_eq!(
            second_page.last().map(String::as_str),
            Some("第「2」页，共「2」页")
        );
        Ok(())
    }

    #[test]
    fn range_beyond_total_is_success_document() -> Result<(), Box<dyn std::error::Error>> {
        let values = prepare_ranking(vec![entry("only", 1)?])?;
        let lines = range_lines(&values, 4, 7, "2023-01-01 00:00:00");
        assert_eq!(lines[1], "未找到第 4-7 名的公开 ranking 数据。");
        Ok(())
    }

    #[test]
    fn timestamp_uses_the_explicit_display_offset() -> Result<(), Box<dyn std::error::Error>> {
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000)?;
        let offset = UtcOffset::from_hms(9, 0, 0)?;
        assert_eq!(format_timestamp(now, offset), "2023-11-15 07:13:20");
        Ok(())
    }
}
