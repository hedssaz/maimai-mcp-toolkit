use std::{sync::Arc, time::Instant};

use tokio::sync::Semaphore;

use crate::{
    b50_image::{B50ImageService, RenderOptions},
    score_service::PlayerScoreService,
};

use super::{
    B50RenderError, B50RenderRequest, B50RenderTimings, RenderedB50, ResourceOverridePolicy, view,
};

const DEFAULT_MAX_RENDER_CONCURRENCY: usize = 2;

pub struct B50RenderService {
    scores: Arc<PlayerScoreService>,
    images: Arc<B50ImageService>,
    paths: ResourceOverridePolicy,
    render_slots: Arc<Semaphore>,
}

impl B50RenderService {
    pub fn new(
        scores: Arc<PlayerScoreService>,
        images: B50ImageService,
        paths: ResourceOverridePolicy,
    ) -> Self {
        Self::with_max_render_concurrency(scores, images, paths, DEFAULT_MAX_RENDER_CONCURRENCY)
    }

    pub fn with_max_render_concurrency(
        scores: Arc<PlayerScoreService>,
        images: B50ImageService,
        paths: ResourceOverridePolicy,
        max_render_concurrency: usize,
    ) -> Self {
        Self::with_shared_images_and_concurrency(
            scores,
            Arc::new(images),
            paths,
            max_render_concurrency,
        )
    }

    pub fn with_shared_images(
        scores: Arc<PlayerScoreService>,
        images: Arc<B50ImageService>,
        paths: ResourceOverridePolicy,
    ) -> Self {
        Self::with_shared_images_and_concurrency(
            scores,
            images,
            paths,
            DEFAULT_MAX_RENDER_CONCURRENCY,
        )
    }

    fn with_shared_images_and_concurrency(
        scores: Arc<PlayerScoreService>,
        images: Arc<B50ImageService>,
        paths: ResourceOverridePolicy,
        max_render_concurrency: usize,
    ) -> Self {
        Self {
            scores,
            images,
            paths,
            render_slots: Arc::new(Semaphore::new(max_render_concurrency.max(1))),
        }
    }

    pub async fn render(&self, request: B50RenderRequest) -> Result<RenderedB50, B50RenderError> {
        let static_dir = self
            .paths
            .static_dir(request.static_dir.clone(), request.style)?;
        let cover_cache_dir = self
            .paths
            .cover_cache_dir(request.cover_cache_dir.clone())?;
        let query_started = Instant::now();
        let response = self
            .scores
            .b50_with_evidence(request.score_request(), false)
            .await?;
        let query = query_started.elapsed();
        let (result, _, _) = response.into_parts();
        let prepare_started = Instant::now();
        let view = view::prepare(&result, request.title)?;
        let prepare = prepare_started.elapsed();
        let filename_stem = match &result.lookup {
            crate::scores::Lookup::Qq(qq) => qq.as_str().to_owned(),
            crate::scores::Lookup::Username(username) => username.as_str().to_owned(),
        };
        let options = RenderOptions {
            style: Some(request.style),
            output_dir: None,
            static_dir,
            cover_cache_dir,
            filename_stem,
        };
        let permit = Arc::clone(&self.render_slots)
            .acquire_owned()
            .await
            .map_err(|_| B50RenderError::TaskJoin)?;
        let images = Arc::clone(&self.images);
        let now = request.now;
        let rendered = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            images.render(&view, options, now)
        })
        .await
        .map_err(|_| B50RenderError::TaskJoin)??;
        Ok(RenderedB50 {
            image_path: rendered.image_path,
            width: rendered.rendered.metadata.width,
            height: rendered.rendered.metadata.height,
            source: result.source,
            local_computed_at: local_computed_at(result.source, request.now),
            timings: B50RenderTimings {
                query,
                prepare,
                draw: rendered.draw_elapsed,
                save: rendered.save_elapsed,
            },
        })
    }
}

pub(super) const fn local_computed_at(
    source: maimai_core::ScoreSource,
    now: time::OffsetDateTime,
) -> Option<time::OffsetDateTime> {
    match source {
        maimai_core::ScoreSource::Local => Some(now),
        maimai_core::ScoreSource::DivingFish
        | maimai_core::ScoreSource::Lxns
        | maimai_core::ScoreSource::OfficialCn => None,
    }
}
