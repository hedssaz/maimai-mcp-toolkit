use std::sync::Arc;

use maimai_catalog::CatalogStore;
use maimai_render::ScoreListRenderer;
use tokio::sync::Semaphore;

use crate::{
    image_output::ImageOutputStore,
    score_service::{PlayerScoreService, ScoreQuery},
};

use super::{ScoreListError, ScoreListImage, ScoreListRequest, prepare};

const DEFAULT_MAX_RENDER_CONCURRENCY: usize = 2;

pub struct ScoreListService {
    catalog: Arc<CatalogStore>,
    scores: Arc<PlayerScoreService>,
    renderer: Arc<ScoreListRenderer>,
    outputs: ImageOutputStore,
    render_slots: Arc<Semaphore>,
}

impl ScoreListService {
    pub fn new(
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
        renderer: ScoreListRenderer,
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
        renderer: ScoreListRenderer,
        outputs: ImageOutputStore,
        max_render_concurrency: usize,
    ) -> Self {
        Self {
            catalog,
            scores,
            renderer: Arc::new(renderer),
            outputs,
            render_slots: render_slots(max_render_concurrency),
        }
    }

    pub async fn render(
        &self,
        request: ScoreListRequest,
    ) -> Result<ScoreListImage, ScoreListError> {
        let mut query = ScoreQuery::new(request.lookup.clone(), request.now.unix_timestamp());
        query.source = request.source;
        let scores = self.scores.records(query).await?;
        let snapshot = self.catalog.snapshot();
        let view = prepare::prepare(&snapshot, &scores, &request.target, request.page)?;
        let permit = Arc::clone(&self.render_slots)
            .acquire_owned()
            .await
            .map_err(|_| ScoreListError::TaskJoin)?;
        let renderer = Arc::clone(&self.renderer);
        let outputs = self.outputs.clone();
        let stem = filename_stem(&request.lookup, &request.target.label());
        let now = request.now;
        let (rendered, saved) = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let rendered = renderer.render(&view)?;
            let saved = outputs.save_png(&stem, &rendered.bytes, now)?;
            Ok::<_, ScoreListError>((rendered, saved))
        })
        .await
        .map_err(|_| ScoreListError::TaskJoin)??;
        Ok(ScoreListImage {
            image_path: saved.path,
            width: rendered.width,
            height: rendered.height,
            source: scores.source,
            placeholder_covers: rendered.placeholder_covers,
        })
    }
}

fn render_slots(maximum: usize) -> Arc<Semaphore> {
    Arc::new(Semaphore::new(maximum.max(1)))
}

fn filename_stem(lookup: &crate::scores::Lookup, target: &str) -> String {
    let player = match lookup {
        crate::scores::Lookup::Qq(value) => value.as_str(),
        crate::scores::Lookup::Username(value) => value.as_str(),
    };
    format!("score_list_{player}_{target}")
}

#[cfg(test)]
mod tests {
    use super::render_slots;

    #[test]
    fn zero_concurrency_is_clamped_to_one_render_slot() {
        assert_eq!(render_slots(0).available_permits(), 1);
        assert_eq!(render_slots(3).available_permits(), 3);
    }
}
