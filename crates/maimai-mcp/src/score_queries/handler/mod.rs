mod api;
mod batch;
mod query;

use std::{future::Future, sync::Arc, time::Duration};

use maimai_app::{
    identity::IdentityDirectory,
    score_service::PlayerScoreService,
    score_settings::ScoreSettingsService,
    scores::{SelectionReason, SourceSelection},
};
use maimai_catalog::CatalogStore;
use maimai_core::ScoreSource;
use maimai_providers::DivingFishClient;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolOutput,
    contract::{ContractError, SurfaceContract},
};

use super::{TOOL_NAMES, error::ScoreQueryToolError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreDeployment {
    Main,
    Public,
}

impl ScoreDeployment {
    const fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }
}

#[derive(Clone)]
pub struct ScoreQueryHandler {
    service: Arc<PlayerScoreService>,
    identities: IdentityDirectory,
    settings: ScoreSettingsService,
    catalog: Arc<CatalogStore>,
    diving_fish: DivingFishClient,
    deployment: ScoreDeployment,
}

impl ScoreQueryHandler {
    pub fn new(
        service: Arc<PlayerScoreService>,
        identities: IdentityDirectory,
        settings: ScoreSettingsService,
        catalog: Arc<CatalogStore>,
        diving_fish: DivingFishClient,
        deployment: ScoreDeployment,
    ) -> Self {
        Self {
            service,
            identities,
            settings,
            catalog,
            diving_fish,
            deployment,
        }
    }

    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        match call.name() {
            "query_b50" => self.query_b50(call.deserialize()?, false).await,
            "query_b50_batch" => self.query_b50_batch(call.deserialize()?).await,
            "query_computed_b50" => self.query_b50(call.deserialize()?, true).await,
            "query_maimai_song_score" => self.song_score(call.deserialize()?).await,
            "query_maimai_player_records" => self.player_records(call.deserialize()?).await,
            "list_diving_fish_apis" => self.list_apis(call.deserialize()?),
            "diving_fish_api" => self.diving_fish_api(call.deserialize()?).await,
            name => Err(ProtocolError::internal(
                format!("unreachable score query route: {name}"),
                None,
            )
            .into()),
        }
    }

    fn visible_selection(&self, selection: SourceSelection) -> SourceSelection {
        if self.deployment.is_public() {
            SourceSelection {
                preferred_source: ScoreSource::DivingFish,
                source: ScoreSource::DivingFish,
                reason: SelectionReason::Default,
            }
        } else {
            selection
        }
    }
}

impl ToolDispatcher for ScoreQueryHandler {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let handler = self.clone();
        async move { handler.dispatch_call(call).await }
    }
}

pub fn score_query_server(
    contract_json: &str,
    handler: ScoreQueryHandler,
) -> Result<ContractServer<ScoreQueryHandler>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&TOOL_NAMES);
    Ok(ContractServer::new(contract, handler))
}

pub(super) async fn within<T>(
    duration: Duration,
    future: impl Future<Output = Result<T, ScoreQueryToolError>>,
) -> Result<T, ScoreQueryToolError> {
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_| ScoreQueryToolError::timeout())?
}

fn clock() -> Result<(i64, String), ScoreQueryToolError> {
    let now = OffsetDateTime::now_utc();
    let rendered = now
        .format(&Rfc3339)
        .map_err(|_| ScoreQueryToolError::internal())?;
    Ok((now.unix_timestamp(), rendered))
}
