use std::sync::Arc;

use maimai_catalog::{CatalogStore, PlateServer};
use maimai_render::CompletionRenderer;
use time::OffsetDateTime;
use tokio::sync::Semaphore;

use crate::{
    image_output::ImageOutputStore,
    score_service::{PlayerScoreService, ScoreQuery},
    scores::PlayerScores,
};

use super::{
    CompletionCapabilities, CompletionError, CompletionIdentity, CompletionImage,
    CompletionItemError, CompletionResponse, LevelProgressRequest, MAX_COMPLETION_BATCH_ITEMS,
    PlateBatchResult, PlateProgressBatchResult, PlateProgressItem, PlateProgressOutput, PlateSpec,
    RatingTableImage, RatingTableRequest, plate, plate_progress, progress, rating,
};

pub struct CompletionService {
    catalog: Arc<CatalogStore>,
    scores: Arc<PlayerScoreService>,
    renderer: Arc<CompletionRenderer>,
    outputs: ImageOutputStore,
    capabilities: CompletionCapabilities,
    render_slots: Arc<Semaphore>,
}

const DEFAULT_MAX_RENDER_CONCURRENCY: usize = 2;

impl CompletionService {
    pub fn new(
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
        renderer: CompletionRenderer,
        outputs: ImageOutputStore,
        capabilities: CompletionCapabilities,
    ) -> Self {
        Self::with_max_render_concurrency(
            catalog,
            scores,
            renderer,
            outputs,
            capabilities,
            DEFAULT_MAX_RENDER_CONCURRENCY,
        )
    }

    pub fn with_max_render_concurrency(
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
        renderer: CompletionRenderer,
        outputs: ImageOutputStore,
        capabilities: CompletionCapabilities,
        max_render_concurrency: usize,
    ) -> Self {
        Self {
            catalog,
            scores,
            renderer: Arc::new(renderer),
            outputs,
            capabilities,
            render_slots: render_slots(max_render_concurrency),
        }
    }

    pub const fn capabilities(&self) -> CompletionCapabilities {
        self.capabilities
    }

    pub async fn render_plate(
        &self,
        identity: CompletionIdentity,
        spec: PlateSpec,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<CompletionResponse<CompletionImage>, CompletionError> {
        let scores = self.records(identity, now).await?;
        let snapshot = self.catalog.snapshot();
        let prepared = plate::prepare(&snapshot, &scores, &spec, self.capabilities.allow_jp)?;
        let image = self
            .render_prepared_plate(1, prepared, filename_stem, now)
            .await?;
        Ok(CompletionResponse {
            value: image,
            source: scores.source,
        })
    }

    pub async fn render_plate_batch(
        &self,
        identity: CompletionIdentity,
        specs: Vec<PlateSpec>,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<PlateBatchResult, CompletionError> {
        validate_batch(&specs)?;
        let scores = self.records(identity, now).await?;
        let snapshot = self.catalog.snapshot();
        let mut result = PlateBatchResult {
            source: Some(scores.source),
            ..PlateBatchResult::default()
        };
        for (offset, spec) in specs.iter().enumerate() {
            let index = offset + 1;
            match plate::prepare(&snapshot, &scores, spec, self.capabilities.allow_jp) {
                Ok(prepared) => match self
                    .render_prepared_plate(index, prepared, filename_stem.clone(), now)
                    .await
                {
                    Ok(image) => result.results.push(image),
                    Err(error) => result.errors.push(item_error(index, spec, error)),
                },
                Err(error) => result.errors.push(item_error(index, spec, error)),
            }
        }
        Ok(result)
    }

    pub async fn render_level_progress(
        &self,
        request: LevelProgressRequest,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<CompletionResponse<CompletionImage>, CompletionError> {
        self.require_server(request.server)?;
        let scores = self.records(request.identity.clone(), now).await?;
        let snapshot = self.catalog.snapshot();
        let view = progress::prepare(&snapshot, &scores, &request)?;
        let permit = Arc::clone(&self.render_slots)
            .acquire_owned()
            .await
            .map_err(|_| CompletionError::TaskJoin)?;
        let renderer = Arc::clone(&self.renderer);
        let outputs = self.outputs.clone();
        let saved = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let rendered = renderer.render_level_progress(&view)?;
            let saved = outputs.save_png(&filename_stem, &rendered.bytes, now)?;
            Ok::<_, CompletionError>((rendered, saved))
        })
        .await
        .map_err(|_| CompletionError::TaskJoin)??;
        let (rendered, saved) = saved;
        Ok(CompletionResponse {
            value: CompletionImage {
                index: 1,
                version: request.level,
                target: request.target.label().to_owned(),
                server: request.server,
                path: saved.path,
                width: rendered.width,
                height: rendered.height,
            },
            source: scores.source,
        })
    }

    pub async fn render_rating_table(
        &self,
        request: RatingTableRequest,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<CompletionResponse<RatingTableImage>, CompletionError> {
        let scores = self.records(request.identity.clone(), now).await?;
        let snapshot = self.catalog.snapshot();
        let view = rating::prepare(&snapshot, &scores, &request, self.capabilities.allow_jp)?;
        let permit = Arc::clone(&self.render_slots)
            .acquire_owned()
            .await
            .map_err(|_| CompletionError::TaskJoin)?;
        let renderer = Arc::clone(&self.renderer);
        let outputs = self.outputs.clone();
        let saved = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let rendered = renderer.render_rating_table(&view)?;
            let saved = outputs.save_png(&filename_stem, &rendered.bytes, now)?;
            Ok::<_, CompletionError>((rendered, saved))
        })
        .await
        .map_err(|_| CompletionError::TaskJoin)??;
        let (rendered, saved) = saved;
        Ok(CompletionResponse {
            value: RatingTableImage {
                path: saved.path,
                width: rendered.width,
                height: rendered.height,
            },
            source: scores.source,
        })
    }

    pub async fn plate_progress(
        &self,
        identity: CompletionIdentity,
        spec: PlateSpec,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<CompletionResponse<PlateProgressOutput>, CompletionError> {
        let scores = self.records(identity, now).await?;
        let snapshot = self.catalog.snapshot();
        let prepared =
            plate_progress::prepare(&snapshot, &scores, &spec, self.capabilities.allow_jp)?;
        let output = self
            .finish_plate_progress(1, prepared, filename_stem, now)
            .await?;
        Ok(CompletionResponse {
            value: output,
            source: scores.source,
        })
    }

    pub async fn plate_progress_batch(
        &self,
        identity: CompletionIdentity,
        specs: Vec<PlateSpec>,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<PlateProgressBatchResult, CompletionError> {
        validate_batch(&specs)?;
        let scores = self.records(identity, now).await?;
        let snapshot = self.catalog.snapshot();
        let mut result = PlateProgressBatchResult {
            source: Some(scores.source),
            ..PlateProgressBatchResult::default()
        };
        for (offset, spec) in specs.iter().enumerate() {
            let index = offset + 1;
            match plate_progress::prepare(&snapshot, &scores, spec, self.capabilities.allow_jp) {
                Ok(prepared) => {
                    let version = prepared.version.clone();
                    let target = prepared.target.label().to_owned();
                    let server = prepared.server;
                    match self
                        .finish_plate_progress(index, prepared, filename_stem.clone(), now)
                        .await
                    {
                        Ok(output) => result.results.push(PlateProgressItem {
                            index,
                            version,
                            target,
                            server,
                            output,
                        }),
                        Err(error) => result.errors.push(item_error(index, spec, error)),
                    }
                }
                Err(error) => result.errors.push(item_error(index, spec, error)),
            }
        }
        Ok(result)
    }

    async fn records(
        &self,
        identity: CompletionIdentity,
        now: OffsetDateTime,
    ) -> Result<PlayerScores, CompletionError> {
        let mut query = ScoreQuery::new(identity.lookup, now.unix_timestamp());
        query.source = self.capabilities.fixed_source.or(identity.source);
        self.scores
            .records(query)
            .await
            .map_err(CompletionError::from)
    }

    async fn render_prepared_plate(
        &self,
        index: usize,
        prepared: plate::PreparedPlate,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<CompletionImage, CompletionError> {
        let permit = Arc::clone(&self.render_slots)
            .acquire_owned()
            .await
            .map_err(|_| CompletionError::TaskJoin)?;
        let renderer = Arc::clone(&self.renderer);
        let outputs = self.outputs.clone();
        let view = prepared.view;
        let saved = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let rendered = renderer.render_plate(&view)?;
            let saved = outputs.save_png(&filename_stem, &rendered.bytes, now)?;
            Ok::<_, CompletionError>((rendered, saved))
        })
        .await
        .map_err(|_| CompletionError::TaskJoin)??;
        let (rendered, saved) = saved;
        Ok(CompletionImage {
            index,
            version: prepared.version,
            target: prepared.target.label().to_owned(),
            server: prepared.server,
            path: saved.path,
            width: rendered.width,
            height: rendered.height,
        })
    }

    async fn finish_plate_progress(
        &self,
        index: usize,
        prepared: plate_progress::PreparedPlateProgress,
        filename_stem: String,
        now: OffsetDateTime,
    ) -> Result<PlateProgressOutput, CompletionError> {
        if prepared.listed_count <= 10 {
            return Ok(PlateProgressOutput::Text(prepared.text));
        }
        let permit = Arc::clone(&self.render_slots)
            .acquire_owned()
            .await
            .map_err(|_| CompletionError::TaskJoin)?;
        let renderer = Arc::clone(&self.renderer);
        let outputs = self.outputs.clone();
        let text = prepared.text;
        let saved = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let rendered = renderer.render_text_panel(&text)?;
            let saved = outputs.save_png(&filename_stem, &rendered.bytes, now)?;
            Ok::<_, CompletionError>((rendered, saved))
        })
        .await
        .map_err(|_| CompletionError::TaskJoin)??;
        let (rendered, saved) = saved;
        Ok(PlateProgressOutput::Image(CompletionImage {
            index,
            version: prepared.version,
            target: prepared.target.label().to_owned(),
            server: prepared.server,
            path: saved.path,
            width: rendered.width,
            height: rendered.height,
        }))
    }

    fn require_server(&self, server: PlateServer) -> Result<(), CompletionError> {
        if server == PlateServer::Jp && !self.capabilities.allow_jp {
            return Err(CompletionError::invalid(
                "当前 surface 不支持日服/dxdata 曲目数据。",
            ));
        }
        Ok(())
    }
}

fn validate_batch(specs: &[PlateSpec]) -> Result<(), CompletionError> {
    if specs.is_empty() {
        return Err(CompletionError::invalid("需要提供 items 或 versions"));
    }
    if specs.len() > MAX_COMPLETION_BATCH_ITEMS {
        return Err(CompletionError::BatchTooLarge);
    }
    Ok(())
}

fn item_error(index: usize, spec: &PlateSpec, error: CompletionError) -> CompletionItemError {
    CompletionItemError {
        index,
        label: format!("{}{}", spec.version.as_str(), spec.target.label()),
        message: error.to_string(),
    }
}

fn render_slots(max_render_concurrency: usize) -> Arc<Semaphore> {
    Arc::new(Semaphore::new(max_render_concurrency.max(1)))
}

#[cfg(test)]
mod tests {
    use super::render_slots;

    #[test]
    fn injected_render_limit_is_explicit_and_never_zero() {
        assert_eq!(render_slots(4).available_permits(), 4);
        assert_eq!(render_slots(0).available_permits(), 1);
    }
}
