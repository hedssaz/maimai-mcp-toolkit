use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
};

use maimai_catalog::CatalogStore;
use maimai_render::RiseScoreRenderer;
use tokio::sync::Semaphore;

use crate::{
    image_output::ImageOutputStore,
    score_service::{PlayerScoreService, ScoreQuery},
};

use super::{RiseScoreError, RiseScoreImage, RiseScoreRequest, prepare, rng::XorShift64};

const DEFAULT_MAX_RENDER_CONCURRENCY: usize = 2;

pub struct RiseScoreService {
    catalog: Arc<CatalogStore>,
    scores: Arc<PlayerScoreService>,
    renderer: Arc<RiseScoreRenderer>,
    outputs: ImageOutputStore,
    render_slots: Arc<Semaphore>,
}

impl RiseScoreService {
    pub fn new(
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
        renderer: RiseScoreRenderer,
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
        renderer: RiseScoreRenderer,
        outputs: ImageOutputStore,
        maximum: usize,
    ) -> Self {
        Self {
            catalog,
            scores,
            renderer: Arc::new(renderer),
            outputs,
            render_slots: Arc::new(Semaphore::new(maximum.max(1))),
        }
    }

    pub async fn render(
        &self,
        request: RiseScoreRequest,
    ) -> Result<RiseScoreImage, RiseScoreError> {
        validate_level(request.level.as_deref())?;
        let mut query = ScoreQuery::new(request.lookup.clone(), request.now.unix_timestamp());
        query.source = request.source;
        let scores = self.scores.records(query).await?;
        let snapshot = self.catalog.snapshot();
        let mut rng = XorShift64::seeded(request_seed(&request));
        let view = prepare::prepare(
            &snapshot,
            &scores,
            request.level.as_deref(),
            request.score,
            request.algorithm,
            &mut rng,
        )?;
        let permit = Arc::clone(&self.render_slots)
            .acquire_owned()
            .await
            .map_err(|_| RiseScoreError::TaskJoin)?;
        let renderer = Arc::clone(&self.renderer);
        let outputs = self.outputs.clone();
        let stem = filename_stem(&request);
        let now = request.now;
        let (rendered, saved) = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let rendered = renderer.render(&view)?;
            let saved = outputs.save_png(&stem, &rendered.bytes, now)?;
            Ok::<_, RiseScoreError>((rendered, saved))
        })
        .await
        .map_err(|_| RiseScoreError::TaskJoin)??;
        Ok(RiseScoreImage {
            image_path: saved.path,
            width: rendered.width,
            height: rendered.height,
            source: scores.source,
            placeholder_covers: rendered.placeholder_covers,
        })
    }
}

fn validate_level(level: Option<&str>) -> Result<(), RiseScoreError> {
    if level.is_some_and(|value| {
        value.trim().is_empty() || value.chars().any(char::is_control) || value.chars().count() > 16
    }) {
        return Err(RiseScoreError::invalid("level 必须是有效等级字符串"));
    }
    Ok(())
}

fn request_seed(request: &RiseScoreRequest) -> u64 {
    let mut hasher = DefaultHasher::new();
    request.lookup.hash(&mut hasher);
    request.now.unix_timestamp_nanos().hash(&mut hasher);
    hasher.finish()
}

fn filename_stem(request: &RiseScoreRequest) -> String {
    let player = match &request.lookup {
        crate::scores::Lookup::Qq(value) => value.as_str(),
        crate::scores::Lookup::Username(value) => value.as_str(),
    };
    format!("rise_score_{player}")
}
