use std::{future::Future, sync::Arc};

use maimai_app::b50_render::B50RenderService;
use time::OffsetDateTime;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolOutput,
    contract::{ContractError, SurfaceContract},
};

use super::{convert, dto::RenderB50Args, error::B50RenderToolError, format};

pub const TOOL_NAME: &str = "render_maimai_b50";
const TOOLS: [&str; 1] = [TOOL_NAME];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderDeployment {
    Main,
    Public,
}

#[derive(Clone)]
pub struct B50RenderDispatcher {
    service: Arc<B50RenderService>,
    deployment: RenderDeployment,
}

impl B50RenderDispatcher {
    pub fn new(service: Arc<B50RenderService>, deployment: RenderDeployment) -> Self {
        Self {
            service,
            deployment,
        }
    }

    async fn dispatch_call_at(
        &self,
        call: ToolCall,
        now: OffsetDateTime,
    ) -> Result<ToolOutput, DispatchError> {
        if call.name() != TOOL_NAME {
            return Err(ProtocolError::internal(
                format!("unreachable B50 render route: {}", call.name()),
                None,
            )
            .into());
        }
        let args: RenderB50Args = call.deserialize()?;
        let prepared = convert::request(args, self.deployment, now)?;
        let result = tokio::time::timeout(prepared.timeout, self.service.render(prepared.request))
            .await
            .map_err(|_| B50RenderToolError::Timeout)?
            .map_err(B50RenderToolError::from)?;
        let text = format::success(&result, self.deployment)?;
        Ok(ToolOutput::text(text))
    }
}

impl ToolDispatcher for B50RenderDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move {
            dispatcher
                .dispatch_call_at(call, OffsetDateTime::now_utc())
                .await
        }
    }
}

pub fn b50_render_server(
    contract_json: &str,
    dispatcher: B50RenderDispatcher,
) -> Result<ContractServer<B50RenderDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&TOOLS);
    Ok(ContractServer::new(contract, dispatcher))
}
