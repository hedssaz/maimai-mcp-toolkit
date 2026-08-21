use std::sync::Arc;

use maimai_catalog::CatalogStore;
use maimai_render::MusicGlobalStatsRenderer;
use time::OffsetDateTime;
use tokio::{sync::Semaphore, task::JoinSet};

use crate::{image_output::ImageOutputStore, music_info::resolve::catalog_variants};

use super::{
    MusicGlobalStatsBatchResult, MusicGlobalStatsError, MusicGlobalStatsFailureKind,
    MusicGlobalStatsImage, MusicGlobalStatsItemError, MusicGlobalStatsRequest,
    resolve::{PreparedMusicGlobalStats, prepare},
};

const DEFAULT_MAX_RENDER_CONCURRENCY: usize = 2;

pub struct MusicGlobalStatsService {
    catalog: Arc<CatalogStore>,
    renderer: Arc<MusicGlobalStatsRenderer>,
    outputs: ImageOutputStore,
    render_slots: Arc<Semaphore>,
}

impl MusicGlobalStatsService {
    pub fn new(
        catalog: Arc<CatalogStore>,
        renderer: MusicGlobalStatsRenderer,
        outputs: ImageOutputStore,
    ) -> Self {
        Self::with_max_render_concurrency(
            catalog,
            renderer,
            outputs,
            DEFAULT_MAX_RENDER_CONCURRENCY,
        )
    }

    pub fn with_max_render_concurrency(
        catalog: Arc<CatalogStore>,
        renderer: MusicGlobalStatsRenderer,
        outputs: ImageOutputStore,
        max_render_concurrency: usize,
    ) -> Self {
        Self {
            catalog,
            renderer: Arc::new(renderer),
            outputs,
            render_slots: Arc::new(Semaphore::new(max_render_concurrency.max(1))),
        }
    }

    pub async fn render(
        &self,
        request: MusicGlobalStatsRequest,
        now: OffsetDateTime,
    ) -> Result<MusicGlobalStatsBatchResult, MusicGlobalStatsError> {
        let snapshot = self.catalog.snapshot();
        let Some((hit, generations)) = catalog_variants(&snapshot, &request.music)? else {
            return Err(
                crate::music_info::MusicInfoError::NotFound(request.music.query_label()).into(),
            );
        };
        let mut prepared = Vec::new();
        let mut errors = Vec::new();
        for (offset, generation) in generations.into_iter().enumerate() {
            let index = offset + 1;
            match prepare(
                &snapshot,
                &request.music,
                &hit,
                generation,
                request.difficulty,
            ) {
                Ok(value) => prepared.push((index, value)),
                Err(error) => errors.push(MusicGlobalStatsItemError {
                    index,
                    query: request.music.query_label(),
                    chart_type: generation_type(generation),
                    kind: MusicGlobalStatsFailureKind::Input,
                    message: error.to_string(),
                }),
            }
        }
        let mut tasks = JoinSet::new();
        for (index, prepared) in prepared {
            let permit = Arc::clone(&self.render_slots)
                .acquire_owned()
                .await
                .map_err(|_| MusicGlobalStatsError::TaskJoin)?;
            let renderer = Arc::clone(&self.renderer);
            let outputs = self.outputs.clone();
            tasks.spawn_blocking(move || {
                let _permit = permit;
                render_one(&renderer, &outputs, index, prepared, now)
            });
        }
        let mut images = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            match joined.map_err(|_| MusicGlobalStatsError::TaskJoin)? {
                Ok(image) => images.push(image),
                Err(error) => errors.push(error),
            }
        }
        images.sort_by_key(|image| image.index);
        errors.sort_by_key(|error| error.index);
        Ok(MusicGlobalStatsBatchResult { images, errors })
    }
}

fn render_one(
    renderer: &MusicGlobalStatsRenderer,
    outputs: &ImageOutputStore,
    index: usize,
    prepared: PreparedMusicGlobalStats,
    now: OffsetDateTime,
) -> Result<MusicGlobalStatsImage, MusicGlobalStatsItemError> {
    let result = (|| {
        let rendered = renderer.render(&prepared.view)?;
        let stem = format!(
            "music_stats_{}_{}_{}",
            prepared.music_id,
            prepared.level_index,
            prepared.chart_type.label()
        );
        let saved = outputs.save_png(&stem, &rendered.bytes, now)?;
        Ok::<_, MusicGlobalStatsError>(MusicGlobalStatsImage {
            index,
            query: prepared.query.clone(),
            music_id: prepared.music_id.clone(),
            title: prepared.title.clone(),
            chart_type: prepared.chart_type,
            level_index: prepared.level_index,
            image_path: saved.path,
            width: rendered.width,
            height: rendered.height,
        })
    })();
    result.map_err(|error| MusicGlobalStatsItemError {
        index,
        query: prepared.query,
        chart_type: Some(prepared.chart_type),
        kind: MusicGlobalStatsFailureKind::Render,
        message: error.to_string(),
    })
}

const fn generation_type(
    generation: maimai_core::ChartGeneration,
) -> Option<crate::music_info::MusicInfoChartType> {
    match generation {
        maimai_core::ChartGeneration::Standard => {
            Some(crate::music_info::MusicInfoChartType::Standard)
        }
        maimai_core::ChartGeneration::Deluxe => Some(crate::music_info::MusicInfoChartType::Deluxe),
        maimai_core::ChartGeneration::UtageOnePlayer
        | maimai_core::ChartGeneration::UtageTwoPlayer => None,
    }
}
