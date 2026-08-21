use std::sync::Arc;

use maimai_catalog::CatalogStore;
use maimai_render::{MusicScoreRenderer, MusicScoreView};
use time::OffsetDateTime;
use tokio::{sync::Semaphore, task::JoinSet};

use crate::{
    image_output::ImageOutputStore,
    music_info::resolve::{catalog_variants, prepare_catalog_variant},
    score_service::{PlayerScoreService, ScoreQuery},
    scores::best_by_chart,
};

use super::{
    MusicScoreBatchResult, MusicScoreError, MusicScoreImage, MusicScoreItemError,
    MusicScoreRequest,
    prepare::{chart_type, view},
};

const DEFAULT_MAX_RENDER_CONCURRENCY: usize = 2;

pub struct MusicScoreService {
    catalog: Arc<CatalogStore>,
    scores: Arc<PlayerScoreService>,
    renderer: Arc<MusicScoreRenderer>,
    outputs: ImageOutputStore,
    render_slots: Arc<Semaphore>,
}

impl MusicScoreService {
    pub fn new(
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
        renderer: MusicScoreRenderer,
        outputs: ImageOutputStore,
    ) -> Self {
        Self::with_max_render_concurrency(
            catalog,
            scores,
            renderer,
            outputs,
            DEFAULT_MAX_RENDER_CONCURRENCY,
        )
    }

    pub fn with_max_render_concurrency(
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
        renderer: MusicScoreRenderer,
        outputs: ImageOutputStore,
        max_render_concurrency: usize,
    ) -> Self {
        Self {
            catalog,
            scores,
            renderer: Arc::new(renderer),
            outputs,
            render_slots: Arc::new(Semaphore::new(max_render_concurrency.max(1))),
        }
    }

    pub async fn render(
        &self,
        request: MusicScoreRequest,
        now: OffsetDateTime,
    ) -> Result<MusicScoreBatchResult, MusicScoreError> {
        let snapshot = self.catalog.snapshot();
        let query_label = request.music.query_label();
        let Some((hit, generations)) = catalog_variants(&snapshot, &request.music)? else {
            return Err(crate::music_info::MusicInfoError::NotFound(query_label).into());
        };
        let mut score_query = ScoreQuery::new(request.player, now.unix_timestamp());
        score_query.source = request.source;
        let scores = self.scores.records(score_query).await?;
        let best = best_by_chart(&scores.records);
        let mut work = Vec::new();
        let mut errors = Vec::new();
        for (offset, generation) in generations.into_iter().enumerate() {
            let index = offset + 1;
            match prepare_catalog_variant(&snapshot, &request.music, &hit, generation).and_then(
                |prepared| {
                    view(&prepared, &best)
                        .map(|view| (prepared.query, prepared.music_id, prepared.title, view))
                        .map_err(|error| {
                            crate::music_info::MusicInfoError::invalid(error.to_string())
                        })
                },
            ) {
                Ok((query, music_id, title, view)) => work.push(RenderWork {
                    index,
                    query,
                    music_id,
                    title,
                    chart_type: chart_type(generation).to_owned(),
                    view,
                }),
                Err(error) => errors.push(MusicScoreItemError {
                    index,
                    query: request.music.query_label(),
                    chart_type: Some(chart_type(generation).to_owned()),
                    message: error.to_string(),
                }),
            }
        }
        let mut tasks = JoinSet::new();
        for work in work {
            let permit = Arc::clone(&self.render_slots)
                .acquire_owned()
                .await
                .map_err(|_| MusicScoreError::TaskJoin)?;
            let renderer = Arc::clone(&self.renderer);
            let outputs = self.outputs.clone();
            tasks.spawn_blocking(move || {
                let _permit = permit;
                render_one(&renderer, &outputs, work, now)
            });
        }
        let mut images = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            match joined.map_err(|_| MusicScoreError::TaskJoin)? {
                Ok(image) => images.push(image),
                Err(error) => errors.push(error),
            }
        }
        images.sort_by_key(|image| image.index);
        errors.sort_by_key(|error| error.index);
        Ok(MusicScoreBatchResult {
            images,
            errors,
            source: scores.source,
        })
    }
}

struct RenderWork {
    index: usize,
    query: String,
    music_id: String,
    title: String,
    chart_type: String,
    view: MusicScoreView,
}

fn render_one(
    renderer: &MusicScoreRenderer,
    outputs: &ImageOutputStore,
    work: RenderWork,
    now: OffsetDateTime,
) -> Result<MusicScoreImage, MusicScoreItemError> {
    let result = (|| {
        let rendered = renderer.render(&work.view)?;
        let stem = format!("music_score_{}_{}", work.music_id, work.chart_type);
        let saved = outputs.save_png(&stem, &rendered.bytes, now)?;
        Ok::<_, MusicScoreError>(MusicScoreImage {
            index: work.index,
            query: work.query.clone(),
            music_id: work.music_id.clone(),
            title: work.title.clone(),
            chart_type: work.chart_type.clone(),
            image_path: saved.path,
            width: rendered.width,
            height: rendered.height,
        })
    })();
    result.map_err(|error| MusicScoreItemError {
        index: work.index,
        query: work.query,
        chart_type: Some(work.chart_type),
        message: error.to_string(),
    })
}
