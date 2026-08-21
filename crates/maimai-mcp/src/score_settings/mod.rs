mod convert;
mod dto;
mod error;
mod format;
#[cfg(test)]
mod tests;

use maimai_app::score_settings::{DeveloperToken, ScoreSettingsService};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolOutput,
    contract::{ContractError, SurfaceContract},
};
use convert::{qq, score_source, source_dto, token_status_dto};
use dto::{BindTokenArgs, ClearTokenDto, EmptyArgs, SwitchSourceArgs};
use error::ScoreSettingsToolError;

pub const MAIN_CONTRACT_JSON: &str = include_str!("../../../../contracts/main/scores.json");
pub const PUBLIC_CONTRACT_JSON: &str = include_str!("../../../../contracts/public/scores.json");
pub const TOOL_NAMES: [&str; 5] = [
    "bind_developer_token",
    "developer_token_status",
    "clear_developer_token",
    "switch_score_source",
    "switch_b50_source",
];

#[derive(Clone, Debug)]
pub struct ScoreSettingsHandler {
    service: ScoreSettingsService,
}

impl ScoreSettingsHandler {
    pub const fn new(service: ScoreSettingsService) -> Self {
        Self { service }
    }

    pub fn handles(tool_name: &str) -> bool {
        TOOL_NAMES.contains(&tool_name)
    }

    pub async fn handle(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        let name = call.name().to_owned();
        let arguments = call.into_arguments();
        match name.as_str() {
            "bind_developer_token" => self.bind_token(arguments).await,
            "developer_token_status" => self.token_status(arguments).await,
            "clear_developer_token" => self.clear_token(arguments).await,
            "switch_score_source" | "switch_b50_source" => self.switch_source(arguments).await,
            _ => Err(ProtocolError::internal(
                format!("unreachable score settings route: {name}"),
                None,
            )
            .into()),
        }
    }

    async fn bind_token(&self, arguments: Map<String, Value>) -> Result<ToolOutput, DispatchError> {
        let args: BindTokenArgs = deserialize(arguments)?;
        let token = DeveloperToken::new(args.developer_token.unwrap_or_default())?;
        let status = self
            .service
            .bind_developer_token(token, time::OffsetDateTime::now_utc())
            .await?;
        Ok(ToolOutput::text(format::token_bound())
            .with_structured_content(to_value(token_status_dto(status))?))
    }

    async fn token_status(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let _: EmptyArgs = deserialize(arguments)?;
        let dto = token_status_dto(self.service.developer_token_status().await?);
        let text = format::token_status(&dto);
        Ok(ToolOutput::text(text).with_structured_content(to_value(dto)?))
    }

    async fn clear_token(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let _: EmptyArgs = deserialize(arguments)?;
        let result = self.service.clear_developer_token().await?;
        let dto = ClearTokenDto {
            bound: result.bound,
            cleared: result.cleared,
        };
        Ok(ToolOutput::text(format::token_cleared(result.cleared))
            .with_structured_content(to_value(dto)?))
    }

    async fn switch_source(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: SwitchSourceArgs = deserialize(arguments)?;
        let setting = self
            .service
            .switch_score_source(qq(args.qq)?, score_source(args.source)?)
            .await?;
        let dto = source_dto(setting, self.service.allows(maimai_core::ScoreSource::Lxns));
        let text = dto.text.clone();
        Ok(ToolOutput::text(text).with_structured_content(to_value(dto)?))
    }
}

impl ToolDispatcher for ScoreSettingsHandler {
    async fn dispatch(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        self.handle(call).await
    }
}

pub fn score_settings_server(
    contract_json: &str,
    handler: ScoreSettingsHandler,
) -> Result<ContractServer<ScoreSettingsHandler>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&TOOL_NAMES);
    Ok(ContractServer::new(contract, handler))
}

fn deserialize<T: DeserializeOwned>(
    arguments: Map<String, Value>,
) -> Result<T, ScoreSettingsToolError> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|_| ScoreSettingsToolError::invalid("工具参数格式不正确。"))
}

fn to_value(value: impl serde::Serialize) -> Result<Value, ScoreSettingsToolError> {
    serde_json::to_value(value).map_err(|_| ScoreSettingsToolError::internal())
}
