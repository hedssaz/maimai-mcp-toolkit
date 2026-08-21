use std::{future::Future, sync::Arc};

use maimai_app::rise_score::RiseScoreService;
use time::OffsetDateTime;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure,
    ToolOutput,
    contract::{ContractError, SurfaceContract},
};

use super::{RiseScoreSurface, convert, dto::RiseScoreArgs, error::RiseScoreToolError, format};

pub const TOOL_NAME: &str = "render_maimai_rise_score";

#[derive(Clone)]
pub struct RiseScoreDispatcher {
    service: Arc<RiseScoreService>,
    surface: RiseScoreSurface,
    now: fn() -> OffsetDateTime,
}

impl RiseScoreDispatcher {
    pub fn new(
        service: Arc<RiseScoreService>,
        surface: RiseScoreSurface,
        now: fn() -> OffsetDateTime,
    ) -> Self {
        Self {
            service,
            surface,
            now,
        }
    }

    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        if call.name() != TOOL_NAME {
            return Err(ProtocolError::internal(
                format!("unreachable rise-score route: {}", call.name()),
                None,
            )
            .into());
        }
        if self.surface == RiseScoreSurface::Public
            && [
                "source",
                "scoreSource",
                "score_source",
                "dataSource",
                "data_source",
            ]
            .iter()
            .any(|name| call.arguments().contains_key(*name))
        {
            return Err(tool_failure(RiseScoreToolError::invalid(
                "公开版 render_maimai_rise_score 不接受来源字段",
            )));
        }
        let args: RiseScoreArgs = call.deserialize()?;
        let request = convert::request(args, self.surface, (self.now)()).map_err(tool_failure)?;
        let result = self
            .service
            .render(request)
            .await
            .map_err(RiseScoreToolError::from)
            .map_err(tool_failure)?;
        Ok(ToolOutput::text(
            format::success(&result, self.surface).map_err(tool_failure)?,
        ))
    }
}

impl ToolDispatcher for RiseScoreDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move { dispatcher.dispatch_call(call).await }
    }
}

pub fn rise_score_server(
    contract_json: &str,
    dispatcher: RiseScoreDispatcher,
) -> Result<ContractServer<RiseScoreDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&[TOOL_NAME]);
    Ok(ContractServer::new(contract, dispatcher))
}

fn tool_failure(error: RiseScoreToolError) -> DispatchError {
    ToolFailure::text(error.to_string()).into()
}
