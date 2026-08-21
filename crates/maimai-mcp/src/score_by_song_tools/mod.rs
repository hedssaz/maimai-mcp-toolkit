mod convert;
mod dto;
mod error;
mod format;
mod serialize;

use maimai_app::score_by_song::ScoreBySongService;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolOutput,
    contract::{ContractError, SurfaceContract},
};

pub const MAIN_CONTRACT_JSON: &str = include_str!("../../../../contracts/main/score_query.json");
pub const PUBLIC_CONTRACT_JSON: &str =
    include_str!("../../../../contracts/public/score_query.json");
pub const TOOL_NAME: &str = "query_maimai_score_by_song";

#[derive(Clone)]
pub struct ScoreBySongDispatcher {
    service: ScoreBySongService,
}

impl ScoreBySongDispatcher {
    pub const fn new(service: ScoreBySongService) -> Self {
        Self { service }
    }

    async fn handle(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        if call.name() != TOOL_NAME {
            return Err(ProtocolError::internal(
                format!("unreachable score-by-song route: {}", call.name()),
                None,
            )
            .into());
        }
        let converted = convert::request(call.deserialize()?)?;
        let result = tokio::time::timeout(converted.timeout, self.service.query(converted.request))
            .await
            .map_err(|_| error::ScoreBySongToolError::timeout())?
            .map_err(error::ScoreBySongToolError::from)?;
        let mut structured = serialize::result(result)?;
        let text = format::text(&structured);
        structured["text"] = serde_json::Value::String(text.clone());
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }
}

impl ToolDispatcher for ScoreBySongDispatcher {
    async fn dispatch(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        self.handle(call).await
    }
}

pub fn score_by_song_server(
    contract_json: &str,
    dispatcher: ScoreBySongDispatcher,
) -> Result<ContractServer<ScoreBySongDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&[TOOL_NAME]);
    Ok(ContractServer::new(contract, dispatcher))
}

#[cfg(test)]
mod tests;
