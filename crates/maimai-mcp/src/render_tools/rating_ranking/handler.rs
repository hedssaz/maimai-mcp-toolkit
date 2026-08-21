use std::{future::Future, sync::Arc};

use maimai_app::rating_ranking::{RatingRankingRequest, RatingRankingService};
use time::OffsetDateTime;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure,
    ToolOutput,
    contract::{ContractError, SurfaceContract},
};

use super::{convert, dto::RatingRankingArgs, format};

pub const TOOL_NAME: &str = "render_maimai_rating_ranking";

#[derive(Clone)]
pub struct RatingRankingDispatcher {
    service: Arc<RatingRankingService>,
    now: fn() -> OffsetDateTime,
}

impl RatingRankingDispatcher {
    pub fn new(service: Arc<RatingRankingService>, now: fn() -> OffsetDateTime) -> Self {
        Self { service, now }
    }

    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        if call.name() != TOOL_NAME {
            return Err(ProtocolError::internal(
                format!("unreachable rating ranking route: {}", call.name()),
                None,
            )
            .into());
        }
        let args: RatingRankingArgs = call.deserialize()?;
        let target = convert::target(args).map_err(tool_error)?;
        let result = self
            .service
            .render(RatingRankingRequest {
                target,
                now: (self.now)(),
            })
            .await
            .map_err(|error| ToolFailure::text(convert::error_text(&error)))?;
        Ok(ToolOutput::text(
            format::image(&result).map_err(tool_error)?,
        ))
    }
}

impl ToolDispatcher for RatingRankingDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move { dispatcher.dispatch_call(call).await }
    }
}

pub fn rating_ranking_server(
    contract_json: &str,
    dispatcher: RatingRankingDispatcher,
) -> Result<ContractServer<RatingRankingDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&[TOOL_NAME]);
    Ok(ContractServer::new(contract, dispatcher))
}

fn tool_error(error: crate::render_tools::RenderToolError) -> DispatchError {
    ToolFailure::text(error.to_string()).into()
}
