use std::{fmt, sync::Arc};

use maimai_catalog::{CatalogSnapshot, CatalogStore};
use maimai_core::ScoreSource;
use maimai_providers::{
    DivingFishCredentials, DivingFishScoreClient, RawJsonPayload,
    lxns_score::{LxnsPlayerScores, LxnsScoreEndpoint},
};
use maimai_storage::StateStore;

use crate::{
    oauth::OAuthService,
    scores::{
        B50Result, Lookup, PlayerScores, RatingMode, SourceSelection, compute_b50_from_records,
        filter_single_song, from_diving_fish_b50, from_diving_fish_records, from_local_records,
        from_lxns_bests, from_lxns_scores, select_source as choose_source,
    },
};

use super::{
    B50Mode, B50Request, PlayerScoreServiceError, PlayerScoreServiceErrorCode, ScoreQuery,
    ScoreServiceResponse, SongScoresRequest,
    helpers::{lxns_song_id, oauth_subject, player_selector, unsupported_source},
    lxns,
};

pub struct PlayerScoreService {
    pub(super) store: StateStore,
    pub(super) catalog: Arc<CatalogStore>,
    diving_fish: DivingFishScoreClient,
    lxns: Option<LxnsScoreAccess>,
    source_policy: ScoreSourcePolicy,
}

pub(super) struct LxnsScoreAccess {
    pub(super) oauth: OAuthService,
    pub(super) endpoint: LxnsScoreEndpoint,
}

#[derive(Clone, Copy, Debug)]
enum ScoreSourcePolicy {
    StoredPreference,
    DivingFishOnly,
}

impl PlayerScoreService {
    pub fn diving_fish_only(
        store: StateStore,
        catalog: Arc<CatalogStore>,
        diving_fish: DivingFishScoreClient,
    ) -> Self {
        Self {
            store,
            catalog,
            diving_fish,
            lxns: None,
            source_policy: ScoreSourcePolicy::DivingFishOnly,
        }
    }

    pub fn with_lxns(
        store: StateStore,
        catalog: Arc<CatalogStore>,
        diving_fish: DivingFishScoreClient,
        oauth: OAuthService,
        endpoint: LxnsScoreEndpoint,
    ) -> Self {
        Self {
            store,
            catalog,
            diving_fish,
            lxns: Some(LxnsScoreAccess { oauth, endpoint }),
            source_policy: ScoreSourcePolicy::StoredPreference,
        }
    }

    pub async fn b50(&self, request: B50Request) -> Result<B50Result, PlayerScoreServiceError> {
        self.b50_with_evidence(request, false)
            .await
            .map(ScoreServiceResponse::into_result)
    }

    pub async fn b50_with_evidence(
        &self,
        request: B50Request,
        include_raw: bool,
    ) -> Result<ScoreServiceResponse<B50Result>, PlayerScoreServiceError> {
        let B50Request { query, mode } = request;
        let selection = self.select_source(&query.lookup, query.source).await?;
        let snapshot = self.catalog.snapshot();
        let (result, raw) = match (selection.source, mode) {
            (ScoreSource::DivingFish, B50Mode::Provider) => {
                let (result, raw) = self.df_b50(&query.lookup, include_raw).await?;
                if result.player.rating == Some(0) {
                    return Err(PlayerScoreServiceError::source_unavailable(
                        "Diving-Fish 当前没有可用成绩",
                    ));
                }
                let normalized = from_diving_fish_b50(result, &snapshot)
                    .map_err(PlayerScoreServiceError::score)?;
                self.cache_diving_fish_b50_best_effort(&normalized, query.now)
                    .await;
                (normalized, raw)
            }
            (ScoreSource::DivingFish, B50Mode::Computed(mode)) => {
                let credentials = self
                    .resolve_df_credentials(query.diving_fish_credentials)
                    .await?;
                let (records, raw) = self
                    .df_records(&query.lookup, credentials, include_raw)
                    .await?;
                let scores = from_diving_fish_records(records, &snapshot)
                    .map_err(PlayerScoreServiceError::score)?;
                self.persist_full_scores(&scores, query.now).await?;
                (
                    compute_b50_from_records(&scores, mode)
                        .map_err(PlayerScoreServiceError::score)?,
                    raw,
                )
            }
            (ScoreSource::Lxns, B50Mode::Provider) => {
                let lxns = self.lxns()?;
                let subject = oauth_subject(&query.lookup)?;
                let (bests, raw) =
                    lxns::bests(&lxns.oauth, &lxns.endpoint, subject, query.now, include_raw)
                        .await?;
                (
                    from_lxns_bests(query.lookup, bests, &snapshot)
                        .map_err(PlayerScoreServiceError::score)?,
                    raw,
                )
            }
            (ScoreSource::Lxns, B50Mode::Computed(mode)) => {
                let (lookup, values, raw) = self
                    .lxns_scores(query.lookup, query.now, include_raw)
                    .await?;
                let scores = from_lxns_scores(lookup, values, &snapshot)
                    .map_err(PlayerScoreServiceError::score)?;
                self.persist_full_scores(&scores, query.now).await?;
                (
                    compute_b50_from_records(&scores, mode)
                        .map_err(PlayerScoreServiceError::score)?,
                    raw,
                )
            }
            (ScoreSource::Local, mode) => {
                let (scores, raw) = self
                    .local_scores(query.lookup, &snapshot, include_raw)
                    .await?;
                let mode = match mode {
                    B50Mode::Provider => RatingMode::Actual,
                    B50Mode::Computed(mode) => mode,
                };
                (
                    compute_b50_from_records(&scores, mode)
                        .map_err(PlayerScoreServiceError::score)?,
                    raw,
                )
            }
            (ScoreSource::OfficialCn, _) => return Err(unsupported_source()),
        };
        Ok(ScoreServiceResponse::new(result, raw, selection))
    }

    pub async fn records(
        &self,
        query: ScoreQuery,
    ) -> Result<PlayerScores, PlayerScoreServiceError> {
        self.records_with_evidence(query, false)
            .await
            .map(ScoreServiceResponse::into_result)
    }

    pub async fn records_with_evidence(
        &self,
        query: ScoreQuery,
        include_raw: bool,
    ) -> Result<ScoreServiceResponse<PlayerScores>, PlayerScoreServiceError> {
        let selection = self.select_source(&query.lookup, query.source).await?;
        let snapshot = self.catalog.snapshot();
        let (result, raw) = match selection.source {
            ScoreSource::DivingFish => {
                let credentials = self
                    .resolve_df_credentials(query.diving_fish_credentials)
                    .await?;
                let (records, raw) = self
                    .df_records(&query.lookup, credentials, include_raw)
                    .await?;
                let scores = from_diving_fish_records(records, &snapshot)
                    .map_err(PlayerScoreServiceError::score)?;
                self.persist_full_scores(&scores, query.now).await?;
                (scores, raw)
            }
            ScoreSource::Lxns => {
                let (lookup, values, raw) = self
                    .lxns_scores(query.lookup, query.now, include_raw)
                    .await?;
                let scores = from_lxns_scores(lookup, values, &snapshot)
                    .map_err(PlayerScoreServiceError::score)?;
                self.persist_full_scores(&scores, query.now).await?;
                (scores, raw)
            }
            ScoreSource::Local => {
                self.local_scores(query.lookup, &snapshot, include_raw)
                    .await?
            }
            ScoreSource::OfficialCn => return Err(unsupported_source()),
        };
        Ok(ScoreServiceResponse::new(result, raw, selection))
    }

    pub async fn song_scores(
        &self,
        request: SongScoresRequest,
    ) -> Result<PlayerScores, PlayerScoreServiceError> {
        self.song_scores_with_evidence(request, false)
            .await
            .map(ScoreServiceResponse::into_result)
    }

    pub async fn song_scores_with_evidence(
        &self,
        request: SongScoresRequest,
        include_raw: bool,
    ) -> Result<ScoreServiceResponse<PlayerScores>, PlayerScoreServiceError> {
        let SongScoresRequest { query, filter } = request;
        let selection = self.select_source(&query.lookup, query.source).await?;
        let snapshot = self.catalog.snapshot();
        let (scores, raw) = match selection.source {
            ScoreSource::DivingFish => {
                let credentials = self
                    .resolve_df_credentials(query.diving_fish_credentials)
                    .await?;
                let (records, raw) = self
                    .df_records(&query.lookup, credentials, include_raw)
                    .await?;
                let scores = from_diving_fish_records(records, &snapshot)
                    .map_err(PlayerScoreServiceError::score)?;
                self.persist_full_scores(&scores, query.now).await?;
                (scores, raw)
            }
            ScoreSource::Lxns => {
                let lxns = self.lxns()?;
                let subject = oauth_subject(&query.lookup)?;
                let song_id = lxns_song_id(&snapshot, &filter.song)?;
                let (result, raw) = lxns::song_bests(
                    &lxns.oauth,
                    &lxns.endpoint,
                    subject,
                    song_id,
                    query.now,
                    include_raw,
                )
                .await?;
                (
                    from_lxns_scores(
                        query.lookup,
                        LxnsPlayerScores {
                            player: result.player,
                            scores: result.scores,
                        },
                        &snapshot,
                    )
                    .map_err(PlayerScoreServiceError::score)?,
                    raw,
                )
            }
            ScoreSource::Local => {
                self.local_scores(query.lookup, &snapshot, include_raw)
                    .await?
            }
            ScoreSource::OfficialCn => return Err(unsupported_source()),
        };
        let records = filter_single_song(&scores, &filter, &snapshot)
            .map_err(PlayerScoreServiceError::score)?;
        Ok(ScoreServiceResponse::new(
            PlayerScores { records, ..scores },
            raw,
            selection,
        ))
    }

    pub async fn select_source(
        &self,
        lookup: &Lookup,
        explicit: Option<ScoreSource>,
    ) -> Result<SourceSelection, PlayerScoreServiceError> {
        if matches!(self.source_policy, ScoreSourcePolicy::DivingFishOnly) {
            if matches!(lookup, Lookup::Qq(_))
                && explicit.is_some_and(|source| source != ScoreSource::DivingFish)
            {
                return Err(PlayerScoreServiceError::source_unavailable(
                    "当前进程仅启用 Diving-Fish 成绩来源",
                ));
            }
            return choose_source(lookup, Some(ScoreSource::DivingFish), None)
                .map_err(PlayerScoreServiceError::score);
        }
        let preference = match lookup {
            Lookup::Qq(qq) => self
                .store
                .score_source_preference(qq)
                .await
                .map_err(|_| PlayerScoreServiceError::storage())?,
            Lookup::Username(_) => None,
        };
        choose_source(lookup, explicit, preference).map_err(PlayerScoreServiceError::score)
    }

    async fn df_b50(
        &self,
        lookup: &Lookup,
        include_raw: bool,
    ) -> Result<(maimai_providers::DivingFishB50, Option<RawJsonPayload>), PlayerScoreServiceError>
    {
        if include_raw {
            let (data, raw) = self
                .diving_fish
                .query_b50_with_raw(player_selector(lookup))
                .await
                .map_err(PlayerScoreServiceError::diving_fish)?
                .into_parts();
            Ok((data, Some(raw)))
        } else {
            self.diving_fish
                .query_b50(player_selector(lookup))
                .await
                .map(|data| (data, None))
                .map_err(PlayerScoreServiceError::diving_fish)
        }
    }

    async fn df_records(
        &self,
        lookup: &Lookup,
        credentials: DivingFishCredentials,
        include_raw: bool,
    ) -> Result<
        (
            maimai_providers::DivingFishPlayerRecords,
            Option<RawJsonPayload>,
        ),
        PlayerScoreServiceError,
    > {
        if include_raw {
            let (data, raw) = self
                .diving_fish
                .query_developer_records_with_raw(player_selector(lookup), credentials)
                .await
                .map_err(PlayerScoreServiceError::diving_fish)?
                .into_parts();
            Ok((data, Some(raw)))
        } else {
            self.diving_fish
                .query_developer_records(player_selector(lookup), credentials)
                .await
                .map(|data| (data, None))
                .map_err(PlayerScoreServiceError::diving_fish)
        }
    }

    async fn lxns_scores(
        &self,
        lookup: Lookup,
        now: i64,
        include_raw: bool,
    ) -> Result<(Lookup, LxnsPlayerScores, Option<RawJsonPayload>), PlayerScoreServiceError> {
        let lxns = self.lxns()?;
        let subject = oauth_subject(&lookup)?;
        let (scores, raw) =
            lxns::scores(&lxns.oauth, &lxns.endpoint, subject, now, include_raw).await?;
        Ok((lookup, scores, raw))
    }

    pub(super) fn lxns(&self) -> Result<&LxnsScoreAccess, PlayerScoreServiceError> {
        self.lxns.as_ref().ok_or_else(|| {
            PlayerScoreServiceError::source_unavailable("当前进程未配置 LXNS 成绩能力")
        })
    }

    async fn local_scores(
        &self,
        lookup: Lookup,
        snapshot: &CatalogSnapshot,
        include_raw: bool,
    ) -> Result<(PlayerScores, Option<RawJsonPayload>), PlayerScoreServiceError> {
        let qq = match &lookup {
            Lookup::Qq(qq) => qq,
            Lookup::Username(_) => {
                return Err(PlayerScoreServiceError::new(
                    PlayerScoreServiceErrorCode::InvalidLookup,
                    "本地成绩只支持 QQ 查询",
                ));
            }
        };
        let records = self
            .store
            .records_for_player(qq)
            .await
            .map_err(|_| PlayerScoreServiceError::storage())?;
        if records.is_empty() {
            return Err(PlayerScoreServiceError::source_unavailable(
                "本地缓存没有可用成绩",
            ));
        }
        let profile = self
            .store
            .profile(qq)
            .await
            .map_err(|_| PlayerScoreServiceError::storage())?;
        let raw = include_raw
            .then(|| {
                RawJsonPayload::from_value(&serde_json::json!({
                    "profile": profile,
                    "records": records,
                }))
            })
            .transpose()
            .map_err(PlayerScoreServiceError::raw_json)?;
        let scores = from_local_records(lookup, &records, profile.as_ref(), snapshot)
            .map_err(PlayerScoreServiceError::score)?;
        Ok((scores, raw))
    }
}

impl fmt::Debug for PlayerScoreService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlayerScoreService")
            .field("catalog", &"CatalogStore")
            .field("diving_fish", &self.diving_fish)
            .field("lxns_configured", &self.lxns.is_some())
            .field("source_policy", &self.source_policy)
            .finish()
    }
}
