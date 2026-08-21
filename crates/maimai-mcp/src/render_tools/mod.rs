pub mod b50;
pub mod completion;
mod convert;
mod dto;
mod error;
mod format;
pub mod global_stats;
pub mod music_score;
pub mod rating_ranking;
pub mod rise_score;
pub mod score_list;
mod score_source;

use std::{future::Future, sync::Arc};

use maimai_app::music_info::MusicInfoService;
use time::OffsetDateTime;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure,
    ToolOutput,
    contract::{ContractError, SurfaceContract},
};
use dto::{MusicInfoArgs, MusicInfoBatchArgs};
pub use error::RenderToolError;

pub const MAIN_CONTRACT_JSON: &str = include_str!("../../../../contracts/main/render.json");
pub const PUBLIC_CONTRACT_JSON: &str = include_str!("../../../../contracts/public/render.json");
pub const TOOL_NAMES: [&str; 2] = ["render_maimai_music_info", "render_maimai_music_info_batch"];

#[derive(Clone)]
pub struct MusicInfoDispatcher {
    service: Arc<MusicInfoService>,
}

impl MusicInfoDispatcher {
    pub fn new(service: Arc<MusicInfoService>) -> Self {
        Self { service }
    }

    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        match call.name() {
            "render_maimai_music_info" => {
                let args: MusicInfoArgs = call.deserialize()?;
                let (request, player) = convert::single(args).map_err(tool_error)?;
                let stem = filename_stem(player.as_ref(), "music_info");
                let result = self
                    .service
                    .render(vec![request], player, stem, OffsetDateTime::now_utc())
                    .await
                    .map_err(|error| tool_error(error.into()))?;
                if let Some(message) = format::single_error(&result) {
                    return Err(ToolFailure::text(message).into());
                }
                let text = format::single(&result).map_err(tool_error)?;
                if result.images.is_empty() {
                    Err(ToolFailure::text(text).into())
                } else {
                    Ok(ToolOutput::text(text))
                }
            }
            "render_maimai_music_info_batch" => {
                let args: MusicInfoBatchArgs = call.deserialize()?;
                let (requests, player) = convert::batch(args).map_err(tool_error)?;
                let stem = filename_stem(player.as_ref(), "music_info_batch");
                let result = self
                    .service
                    .render(requests, player, stem, OffsetDateTime::now_utc())
                    .await
                    .map_err(|error| tool_error(error.into()))?;
                let text = format::batch(&result).map_err(tool_error)?;
                if result.images.is_empty() && !result.errors.is_empty() {
                    Err(ToolFailure::text(text).into())
                } else {
                    Ok(ToolOutput::text(text))
                }
            }
            name => Err(
                ProtocolError::internal(format!("unreachable render route: {name}"), None).into(),
            ),
        }
    }
}

impl ToolDispatcher for MusicInfoDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move { dispatcher.dispatch_call(call).await }
    }
}

pub fn music_info_server(
    contract_json: &str,
    dispatcher: MusicInfoDispatcher,
) -> Result<ContractServer<MusicInfoDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&TOOL_NAMES);
    Ok(ContractServer::new(contract, dispatcher))
}

fn filename_stem(player: Option<&maimai_app::scores::Lookup>, fallback: &str) -> String {
    match player {
        Some(maimai_app::scores::Lookup::Qq(value)) => value.as_str().to_owned(),
        Some(maimai_app::scores::Lookup::Username(value)) => value.as_str().to_owned(),
        None => fallback.to_owned(),
    }
}

fn tool_error(error: RenderToolError) -> DispatchError {
    ToolFailure::text(error.to_string()).into()
}

#[cfg(test)]
mod tests;
