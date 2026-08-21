use std::sync::Arc;

use maimai_app::{
    b50_image::{B50ImageService, B50ImageStyle, ResourceDirectories, StyleStore},
    b50_render::{B50RenderService, ResourceOverridePolicy},
    catalog_refresh::{CatalogRefreshService, EnabledSources, job::CatalogRefreshJobs},
    completion::{CompletionCapabilities, CompletionService},
    identity::IdentityService,
    image_output::{ImageOutputPolicy, ImageOutputStore},
    music_global_stats::MusicGlobalStatsService,
    music_info::MusicInfoService,
    music_score::MusicScoreService,
    oauth::OAuthService,
    rankings::RankingService,
    rating_ranking::RatingRankingService,
    rise_score::RiseScoreService,
    score_by_song::ScoreBySongService,
    score_list::ScoreListService,
    score_service::PlayerScoreService,
    score_settings::{AllowedScoreSources, ScoreSettingsService},
};
use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_providers::{
    CatalogSourceClient, CatalogSourceConfig, DivingFishClient, DivingFishScoreClient, NapCatClient,
};
use maimai_render::{
    CompletionRenderer, LegacyAssets, LegacyRenderer, MusicGlobalStatsRenderer, MusicInfoRenderer,
    MusicScoreRenderer, RatingRankingRenderer, RiseScoreRenderer, ScoreListRenderer,
};
use maimai_storage::{CatalogRefreshJobStore, StateStore};
use reqwest::redirect::Policy;
use thiserror::Error;

use super::config::RuntimeConfig;

pub(crate) struct SharedServices {
    pub catalog: Arc<CatalogStore>,
    pub state: StateStore,
    pub catalog_refresh: Arc<CatalogRefreshService>,
    pub catalog_jobs: Arc<CatalogRefreshJobs>,
    pub identity: IdentityService,
    pub oauth: OAuthService,
    pub rankings: RankingService,
    pub scores: Arc<PlayerScoreService>,
    pub score_settings: ScoreSettingsService,
    pub score_by_song: ScoreBySongService,
    pub diving_fish: DivingFishClient,
    pub render: RenderServices,
    pub display_offset: time::UtcOffset,
}

pub(super) type PublicServices = SharedServices;

pub(crate) struct RenderServices {
    pub b50: Arc<B50RenderService>,
    pub completion: Arc<CompletionService>,
    pub music_info: Arc<MusicInfoService>,
    pub music_score: Arc<MusicScoreService>,
    pub global_stats: Arc<MusicGlobalStatsService>,
    pub rating_ranking: Arc<RatingRankingService>,
    pub score_list: Arc<ScoreListService>,
    pub rise_score: Arc<RiseScoreService>,
}

struct LoadedRenderers {
    outputs: ImageOutputStore,
    b50_images: Arc<B50ImageService>,
    b50_paths: ResourceOverridePolicy,
    completion: CompletionRenderer,
    music_info: MusicInfoRenderer,
    music_score: MusicScoreRenderer,
    global_stats: MusicGlobalStatsRenderer,
    rating_ranking: RatingRankingRenderer,
    score_list: ScoreListRenderer,
    rise_score: RiseScoreRenderer,
}

impl SharedServices {
    pub(super) async fn open_public(config: RuntimeConfig) -> Result<Self, PublicServiceError> {
        Self::open(config).await
    }

    async fn open(config: RuntimeConfig) -> Result<Self, PublicServiceError> {
        let http = reqwest::Client::builder()
            .user_agent(format!("maimai-public/{}", env!("CARGO_PKG_VERSION")))
            .redirect(Policy::none())
            .build()?;
        let diving_fish = DivingFishClient::with_http_client(
            http,
            &config.diving_fish_api_base_url,
            &config.diving_fish_cover_base_url,
        )?;
        let score_client = DivingFishScoreClient::new(diving_fish.clone());
        let napcat = NapCatClient::new(config.napcat.clone())?;
        let catalog_source = Arc::new(CatalogSourceClient::new(CatalogSourceConfig::default())?);
        let renderers = LoadedRenderers::load(&config)?;

        let catalog = Arc::new(
            CatalogStore::load(CatalogFiles::public_from_data_dir(config.paths.data_dir())).await?,
        );
        let state = StateStore::open(config.paths.state_db()).await?;
        let identity = IdentityService::new(state.clone(), napcat.clone());
        let identities = identity.directory().clone();
        let oauth = match config.oauth_client {
            Some(client) => OAuthService::new(state.clone(), client),
            None => OAuthService::without_client(state.clone()),
        };
        let score_settings =
            ScoreSettingsService::new(state.clone(), AllowedScoreSources::public());
        let scores = Arc::new(PlayerScoreService::diving_fish_only(
            state.clone(),
            Arc::clone(&catalog),
            score_client.clone(),
        ));
        let rankings = RankingService::new(
            state.clone(),
            napcat,
            identity.clone(),
            Arc::clone(&scores),
            Arc::clone(&catalog),
        );
        let score_by_song =
            ScoreBySongService::new(identities, Arc::clone(&catalog), Arc::clone(&scores));
        let render = RenderServices::new(
            Arc::clone(&catalog),
            Arc::clone(&scores),
            score_client,
            config.display_offset,
            CompletionCapabilities::public(),
            renderers,
        );
        let catalog_refresh = Arc::new(CatalogRefreshService::new(
            catalog_source,
            Arc::clone(&catalog),
            EnabledSources::public(),
        )?);
        let catalog_store = match CatalogRefreshJobStore::open(config.paths.state_db()).await {
            Ok(store) => store,
            Err(error) => {
                state.close().await;
                return Err(error.into());
            }
        };
        let catalog_jobs =
            match CatalogRefreshJobs::open(Arc::clone(&catalog_refresh), catalog_store).await {
                Ok(jobs) => Arc::new(jobs),
                Err(error) => {
                    state.close().await;
                    return Err(error.into());
                }
            };

        Ok(Self {
            catalog,
            state,
            catalog_refresh,
            catalog_jobs,
            identity,
            oauth,
            rankings,
            scores,
            score_settings,
            score_by_song,
            diving_fish,
            render,
            display_offset: config.display_offset,
        })
    }
}

impl LoadedRenderers {
    fn load(config: &RuntimeConfig) -> Result<Self, PublicServiceError> {
        let outputs =
            ImageOutputStore::new(config.output_root.clone(), ImageOutputPolicy::standard())?;
        let cover_guard = ImageOutputStore::new(
            config.cover_cache_root.clone(),
            ImageOutputPolicy::standard(),
        )?;
        let legacy_font = config.static_root.join("adobe_simhei.otf");
        let legacy = LegacyRenderer::new(LegacyAssets::new(&legacy_font, &legacy_font))?;
        let directories =
            ResourceDirectories::new(config.static_root.clone(), config.cover_cache_root.clone())?;
        let styles = StyleStore::new(config.style_config.clone(), B50ImageStyle::Yuzu)?;
        let _style = styles.current()?;
        let b50_images = Arc::new(B50ImageService::new(
            styles,
            outputs.clone().into(),
            legacy,
            directories,
        ));
        let completion = CompletionRenderer::new(&config.static_root, &config.cover_cache_root)?;
        let music_info = MusicInfoRenderer::new(&config.static_root, &config.cover_cache_root)?;
        let music_score = MusicScoreRenderer::new(&config.static_root, &config.cover_cache_root)?;
        let global_stats = MusicGlobalStatsRenderer::new(&config.static_root)?;
        let rating_ranking = RatingRankingRenderer::new(&config.static_root)?;
        let score_list = ScoreListRenderer::new(&config.static_root, &config.cover_cache_root)?;
        let rise_score = RiseScoreRenderer::new(&config.static_root, &config.cover_cache_root)?;
        cover_guard.ensure_root()?;
        let b50_paths = ResourceOverridePolicy::new(
            config.static_root.clone(),
            [config.static_root.clone()],
            [cover_guard.root().to_owned()],
        )?;
        Ok(Self {
            outputs,
            b50_images,
            b50_paths,
            completion,
            music_info,
            music_score,
            global_stats,
            rating_ranking,
            score_list,
            rise_score,
        })
    }
}

impl RenderServices {
    fn new(
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
        score_client: DivingFishScoreClient,
        display_offset: time::UtcOffset,
        completion_capabilities: CompletionCapabilities,
        loaded: LoadedRenderers,
    ) -> Self {
        Self {
            b50: Arc::new(B50RenderService::with_shared_images(
                Arc::clone(&scores),
                Arc::clone(&loaded.b50_images),
                loaded.b50_paths,
            )),
            completion: Arc::new(CompletionService::new(
                Arc::clone(&catalog),
                Arc::clone(&scores),
                loaded.completion,
                loaded.outputs.clone(),
                completion_capabilities,
            )),
            music_info: Arc::new(MusicInfoService::new(
                Arc::clone(&catalog),
                Arc::clone(&scores),
                loaded.music_info,
                loaded.outputs.clone(),
            )),
            music_score: Arc::new(MusicScoreService::new(
                Arc::clone(&catalog),
                Arc::clone(&scores),
                loaded.music_score,
                loaded.outputs.clone(),
            )),
            global_stats: Arc::new(MusicGlobalStatsService::new(
                Arc::clone(&catalog),
                loaded.global_stats,
                loaded.outputs.clone(),
            )),
            rating_ranking: Arc::new(RatingRankingService::new(
                score_client,
                loaded.rating_ranking,
                loaded.outputs.clone(),
                display_offset,
            )),
            score_list: Arc::new(ScoreListService::new(
                Arc::clone(&catalog),
                Arc::clone(&scores),
                loaded.score_list,
                loaded.outputs.clone(),
            )),
            rise_score: Arc::new(RiseScoreService::new(
                catalog,
                scores,
                loaded.rise_score,
                loaded.outputs,
            )),
        }
    }
}

#[derive(Debug, Error)]
pub enum PublicServiceError {
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Provider(#[from] maimai_providers::ProviderError),
    #[error(transparent)]
    NapCat(#[from] maimai_providers::NapCatError),
    #[error(transparent)]
    CatalogSource(#[from] maimai_providers::CatalogSourceError),
    #[error(transparent)]
    Catalog(#[from] maimai_catalog::CatalogStoreError),
    #[error(transparent)]
    Storage(#[from] maimai_storage::StorageError),
    #[error(transparent)]
    Refresh(#[from] maimai_app::catalog_refresh::RefreshError),
    #[error(transparent)]
    RefreshJob(#[from] maimai_app::catalog_refresh::job::RefreshJobError),
    #[error(transparent)]
    Render(#[from] maimai_render::RenderError),
    #[error(transparent)]
    B50Image(#[from] maimai_app::b50_image::B50ImageError),
    #[error(transparent)]
    B50Render(#[from] maimai_app::b50_render::B50RenderError),
    #[error(transparent)]
    ImageOutput(#[from] maimai_app::image_output::ImageOutputError),
    #[error(transparent)]
    ScoreListRender(#[from] maimai_render::ScoreListRenderError),
    #[error(transparent)]
    RiseScoreRender(#[from] maimai_render::RiseScoreRenderError),
}

#[cfg(test)]
mod tests;
