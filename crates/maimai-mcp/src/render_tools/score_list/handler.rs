use std::{future::Future, sync::Arc};

use maimai_app::score_list::ScoreListService;
use time::OffsetDateTime;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure,
    ToolOutput,
    contract::{ContractError, SurfaceContract},
};

use super::{ScoreListSurface, convert, dto::ScoreListArgs, error::ScoreListToolError, format};

pub const TOOL_NAME: &str = "render_maimai_score_list";

#[derive(Clone)]
pub struct ScoreListDispatcher {
    service: Arc<ScoreListService>,
    surface: ScoreListSurface,
    now: fn() -> OffsetDateTime,
}

impl ScoreListDispatcher {
    pub fn new(
        service: Arc<ScoreListService>,
        surface: ScoreListSurface,
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
                format!("unreachable score-list route: {}", call.name()),
                None,
            )
            .into());
        }
        if self.surface == ScoreListSurface::Public
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
            return Err(tool_failure(ScoreListToolError::invalid(
                "公开版 render_maimai_score_list 不接受来源字段",
            )));
        }
        let args: ScoreListArgs = call.deserialize()?;
        let request = convert::request(args, self.surface, (self.now)()).map_err(tool_failure)?;
        let result = self
            .service
            .render(request)
            .await
            .map_err(ScoreListToolError::from)
            .map_err(tool_failure)?;
        Ok(ToolOutput::text(
            format::success(&result, self.surface).map_err(tool_failure)?,
        ))
    }
}

impl ToolDispatcher for ScoreListDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move { dispatcher.dispatch_call(call).await }
    }
}

pub fn score_list_server(
    contract_json: &str,
    dispatcher: ScoreListDispatcher,
) -> Result<ContractServer<ScoreListDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&[TOOL_NAME]);
    Ok(ContractServer::new(contract, dispatcher))
}

fn tool_failure(error: ScoreListToolError) -> DispatchError {
    ToolFailure::text(error.to_string()).into()
}
