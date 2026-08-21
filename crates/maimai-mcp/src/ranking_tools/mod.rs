mod b50;
mod convert;
mod dto;
mod error;
mod format;
mod song;
mod status;

use maimai_app::rankings::RankingService;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use time::UtcOffset;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolOutput,
    contract::{ContractError, SurfaceContract},
};

pub const MAIN_CONTRACT_JSON: &str = include_str!("../../../../contracts/main/rankings.json");
pub const PUBLIC_CONTRACT_JSON: &str = include_str!("../../../../contracts/public/rankings.json");
pub const TOOL_NAMES: [&str; 11] = [
    "group_b50_report",
    "group_b50_cache_status",
    "group_b50_job_status",
    "group_b50_member_rank",
    "group_b50_rank_at",
    "clear_group_b50_cache",
    "group_song_score_report",
    "group_song_score_member_rank",
    "group_song_score_cache_status",
    "group_song_score_job_status",
    "clear_group_song_score_cache",
];

#[derive(Clone)]
pub struct RankingDispatcher {
    pub(super) service: RankingService,
    pub(super) display_offset: UtcOffset,
}

impl RankingDispatcher {
    pub const fn new(service: RankingService, display_offset: UtcOffset) -> Self {
        Self {
            service,
            display_offset,
        }
    }

    pub async fn handle(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        let name = call.name().to_owned();
        let arguments = call.into_arguments();
        match name.as_str() {
            "group_b50_report" => self.b50_report(arguments).await,
            "group_b50_cache_status" => self.b50_cache_status(arguments).await,
            "group_b50_job_status" => self.b50_job_status(arguments).await,
            "group_b50_member_rank" => self.b50_member_rank(arguments).await,
            "group_b50_rank_at" => self.b50_rank_at(arguments).await,
            "clear_group_b50_cache" => self.clear_b50(arguments).await,
            "group_song_score_report" => self.song_report(arguments).await,
            "group_song_score_member_rank" => self.song_member_rank(arguments).await,
            "group_song_score_cache_status" => self.song_cache_status(arguments).await,
            "group_song_score_job_status" => self.song_job_status(arguments).await,
            "clear_group_song_score_cache" => self.clear_song(arguments).await,
            _ => Err(
                ProtocolError::internal(format!("unreachable ranking route: {name}"), None).into(),
            ),
        }
    }
}

impl ToolDispatcher for RankingDispatcher {
    async fn dispatch(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        self.handle(call).await
    }
}

pub fn rankings_server(
    contract_json: &str,
    dispatcher: RankingDispatcher,
) -> Result<ContractServer<RankingDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&TOOL_NAMES);
    Ok(ContractServer::new(contract, dispatcher))
}

fn deserialize<T: DeserializeOwned>(
    arguments: Map<String, Value>,
) -> Result<T, error::RankingToolError> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|_| error::RankingToolError::invalid("工具参数格式不正确。"))
}

fn output(text: String, structured: Value) -> Result<ToolOutput, DispatchError> {
    Ok(ToolOutput::text(text).with_structured_content(structured))
}

#[cfg(test)]
mod tests;
