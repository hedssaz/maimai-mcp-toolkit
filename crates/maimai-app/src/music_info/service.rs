use std::{collections::BTreeMap, sync::Arc};

use maimai_catalog::CatalogStore;
use maimai_render::{MusicInfoRenderer, PlayerSongScoreContext};
use time::OffsetDateTime;
use tokio::task::JoinSet;

use crate::{
    image_output::ImageOutputStore,
    score_service::{B50Mode, B50Request, PlayerScoreService, ScoreQuery},
    scores::{B50Chart, B50Result, Lookup},
};

use super::{
    MAX_MUSIC_INFO_BATCH_ITEMS, MusicInfoBatchResult, MusicInfoError, MusicInfoImage,
    MusicInfoItemError, MusicInfoRequest,
    resolve::{PreparedMusicInfo, resolve},
};

const MAX_RENDER_CONCURRENCY: usize = 4;

pub struct MusicInfoService {
    catalog: Arc<CatalogStore>,
    scores: Arc<PlayerScoreService>,
    renderer: Arc<MusicInfoRenderer>,
    outputs: ImageOutputStore,
}

impl MusicInfoService {
    pub fn new(
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
        renderer: MusicInfoRenderer,
        outputs: ImageOutputStore,
    ) -> Self {
        Self {
            catalog,
            scores,
            renderer: Arc::new(renderer),
            outputs,
        }
    }

    pub async fn render(
        &self,
        requests: Vec<MusicInfoRequest>,
        player: Option<Lookup>,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<MusicInfoBatchResult, MusicInfoError> {
        if requests.is_empty() {
            return Err(MusicInfoError::invalid("需要提供 items 或 queries"));
        }
        if requests.len() > MAX_MUSIC_INFO_BATCH_ITEMS {
            return Err(MusicInfoError::BatchTooLarge);
        }
        let snapshot = self.catalog.snapshot();
        let mut work = Vec::new();
        let mut errors = Vec::new();
        for (offset, request) in requests.iter().enumerate() {
            let index = offset + 1;
            match resolve(&snapshot, request) {
                Ok(variants) => {
                    for (variant_offset, prepared) in variants.into_iter().enumerate() {
                        work.push(RenderWork {
                            index,
                            sub_index: variant_offset + 1,
                            prepared,
                        });
                    }
                }
                Err(error) => errors.push(MusicInfoItemError {
                    index,
                    sub_index: None,
                    query: request.query_label(),
                    chart_type: request.chart_type,
                    message: error.to_string(),
                }),
            }
        }
        let player = match player {
            Some(lookup) => self.player_b50(lookup, now).await.ok(),
            None => None,
        };
        for item in &mut work {
            let context = player
                .as_ref()
                .map(|player| score_context(player, &item.prepared))
                .transpose()?;
            item.prepared.view = item.prepared.view.clone().with_score_context(context);
        }
        let rendered = self.render_bounded(work, filename_stem, now).await?;
        let mut images = Vec::new();
        for result in rendered {
            match result {
                Ok(image) => images.push(image),
                Err(error) => errors.push(error),
            }
        }
        errors.sort_by_key(|error| (error.index, error.chart_type));
        Ok(MusicInfoBatchResult { images, errors })
    }

    async fn player_b50(
        &self,
        lookup: Lookup,
        now: OffsetDateTime,
    ) -> Result<B50Result, crate::score_service::PlayerScoreServiceError> {
        self.scores
            .b50(B50Request {
                query: ScoreQuery::new(lookup, now.unix_timestamp()),
                mode: B50Mode::Provider,
            })
            .await
    }

    async fn render_bounded(
        &self,
        work: Vec<RenderWork>,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<Vec<Result<MusicInfoImage, MusicInfoItemError>>, MusicInfoError> {
        let count = work.len();
        let mut ordered = vec![None; count];
        let mut pending = work.into_iter().enumerate();
        let mut tasks = JoinSet::new();
        for _ in 0..MAX_RENDER_CONCURRENCY {
            spawn_next(
                &mut tasks,
                &mut pending,
                Arc::clone(&self.renderer),
                self.outputs.clone(),
                filename_stem.clone(),
                now,
            );
        }
        while let Some(joined) = tasks.join_next().await {
            let (position, result) = joined.map_err(|_| MusicInfoError::TaskJoin)?;
            if let Some(slot) = ordered.get_mut(position) {
                *slot = Some(result);
            }
            spawn_next(
                &mut tasks,
                &mut pending,
                Arc::clone(&self.renderer),
                self.outputs.clone(),
                filename_stem.clone(),
                now,
            );
        }
        ordered
            .into_iter()
            .map(|result| result.ok_or(MusicInfoError::TaskJoin))
            .collect()
    }
}

struct RenderWork {
    index: usize,
    sub_index: usize,
    prepared: PreparedMusicInfo,
}

fn spawn_next(
    tasks: &mut JoinSet<(usize, Result<MusicInfoImage, MusicInfoItemError>)>,
    pending: &mut impl Iterator<Item = (usize, RenderWork)>,
    renderer: Arc<MusicInfoRenderer>,
    outputs: ImageOutputStore,
    filename_stem: String,
    now: OffsetDateTime,
) {
    let Some((position, work)) = pending.next() else {
        return;
    };
    tasks.spawn_blocking(move || {
        let result = render_one(&renderer, &outputs, &filename_stem, now, work);
        (position, result)
    });
}

fn render_one(
    renderer: &MusicInfoRenderer,
    outputs: &ImageOutputStore,
    filename_stem: &str,
    now: OffsetDateTime,
    work: RenderWork,
) -> Result<MusicInfoImage, MusicInfoItemError> {
    let chart_type = work.prepared.chart_type;
    let result = (|| {
        let rendered = renderer.render(&work.prepared.view)?;
        let saved = outputs.save_png(filename_stem, &rendered.bytes, now)?;
        Ok::<_, MusicInfoError>(MusicInfoImage {
            index: work.index,
            sub_index: work.sub_index,
            query: work.prepared.query.clone(),
            music_id: work.prepared.music_id.clone(),
            title: work.prepared.title.clone(),
            chart_type,
            image_path: saved.path,
            width: rendered.width,
            height: rendered.height,
        })
    })();
    result.map_err(|error| MusicInfoItemError {
        index: work.index,
        sub_index: Some(work.sub_index),
        query: work.prepared.query,
        chart_type,
        message: error.to_string(),
    })
}

fn score_context(
    player: &B50Result,
    prepared: &PreparedMusicInfo,
) -> Result<PlayerSongScoreContext, MusicInfoError> {
    let (section, capacity) = if prepared.current {
        (player.b15.as_slice(), 15)
    } else {
        (player.b35.as_slice(), 35)
    };
    let mut chart_ratings = BTreeMap::<maimai_core::Difficulty, u32>::new();
    for chart in section
        .iter()
        .filter(|chart| prepared.chart_keys.contains(&chart.key))
    {
        if let Some(rating) = chart.rating {
            chart_ratings
                .entry(chart.key.difficulty())
                .and_modify(|current| *current = (*current).max(rating))
                .or_insert(rating);
        }
    }
    let floor = (section.len() >= capacity)
        .then(|| section.iter().filter_map(chart_rating).min())
        .flatten();
    PlayerSongScoreContext::new(
        player.player.rating.or(player.player.actual_rating),
        capacity,
        section.len().min(capacity),
        floor,
        chart_ratings,
    )
    .map_err(MusicInfoError::from)
}

fn chart_rating(chart: &B50Chart) -> Option<u32> {
    chart.rating
}
