use std::{future::Future, sync::Arc};

use maimai_app::music_global_stats::MusicGlobalStatsService;
use time::OffsetDateTime;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure,
    ToolOutput,
    contract::{ContractError, SurfaceContract},
};

use super::{convert, dto::MusicGlobalStatsArgs, format};

pub const TOOL_NAME: &str = "render_maimai_music_global_stats";

#[derive(Clone)]
pub struct MusicGlobalStatsDispatcher {
    service: Arc<MusicGlobalStatsService>,
}

impl MusicGlobalStatsDispatcher {
    pub fn new(service: Arc<MusicGlobalStatsService>) -> Self {
        Self { service }
    }

    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        if call.name() != TOOL_NAME {
            return Err(ProtocolError::internal(
                format!("unreachable music global stats route: {}", call.name()),
                None,
            )
            .into());
        }
        let args: MusicGlobalStatsArgs = call.deserialize()?;
        let request = convert::request(args).map_err(tool_error)?;
        let result = self
            .service
            .render(request, OffsetDateTime::now_utc())
            .await
            .map_err(|error| ToolFailure::text(convert::error_text(&error)))?;
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
}

impl ToolDispatcher for MusicGlobalStatsDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move { dispatcher.dispatch_call(call).await }
    }
}

pub fn music_global_stats_server(
    contract_json: &str,
    dispatcher: MusicGlobalStatsDispatcher,
) -> Result<ContractServer<MusicGlobalStatsDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&[TOOL_NAME]);
    Ok(ContractServer::new(contract, dispatcher))
}

fn tool_error(error: crate::render_tools::RenderToolError) -> DispatchError {
    ToolFailure::text(error.to_string()).into()
}
