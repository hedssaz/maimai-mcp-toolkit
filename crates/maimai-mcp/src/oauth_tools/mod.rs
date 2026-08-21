mod convert;
mod dto;
mod error;
mod format;
#[cfg(test)]
mod tests;

use std::time::Duration;

use maimai_app::oauth::{OAuthService, PreparePokeTiming};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolOutput,
    contract::{ContractError, SurfaceContract},
};
use convert::{
    authorization_dto, bound_dto, confirm_dto, confirm_value, optional_context, pending_dto,
    required_code, required_context, status_dto, subject, to_value, trusted_state, unbind_dto,
};
use dto::{BindCodeArgs, ConfirmPokeArgs, OAuthUrlArgs, PreparePokeArgs, SubjectArgs};
use error::OAuthToolError;

pub const PUBLIC_CONTRACT_JSON: &str = include_str!("../../../../contracts/public/oauth.json");
pub const TOOL_NAMES: [&str; 6] = [
    "maimai_lxns_oauth_url",
    "maimai_lxns_bind_code",
    "maimai_lxns_prepare_poke",
    "maimai_lxns_confirm_poke",
    "maimai_lxns_status",
    "maimai_lxns_unbind",
];

#[derive(Clone)]
pub struct OAuthDispatcher {
    service: OAuthService,
    surface: OAuthSurface,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OAuthSurface {
    Main,
    Public,
}

impl OAuthDispatcher {
    pub const fn new(service: OAuthService) -> Self {
        Self {
            service,
            surface: OAuthSurface::Public,
        }
    }

    pub const fn main(service: OAuthService) -> Self {
        Self {
            service,
            surface: OAuthSurface::Main,
        }
    }

    pub fn handles(tool_name: &str) -> bool {
        TOOL_NAMES.contains(&tool_name)
    }
}

impl ToolDispatcher for OAuthDispatcher {
    async fn dispatch(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        self.dispatch_call(call).await
    }
}

pub fn oauth_server(
    contract_json: &str,
    dispatcher: OAuthDispatcher,
) -> Result<ContractServer<OAuthDispatcher>, ContractError> {
    Ok(ContractServer::new(
        SurfaceContract::parse(contract_json)?,
        dispatcher,
    ))
}

impl OAuthDispatcher {
    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        let name = call.name().to_owned();
        let mut arguments = call.into_arguments();
        match name.as_str() {
            "maimai_lxns_oauth_url" => self.authorization_url(arguments).await,
            "maimai_lxns_bind_code" => {
                let timeout = self.exchange_timeout(&mut arguments)?;
                self.bind_code(arguments, timeout).await
            }
            "maimai_lxns_prepare_poke" => {
                let timeout = self.exchange_timeout(&mut arguments)?;
                self.prepare_poke(arguments, timeout).await
            }
            "maimai_lxns_confirm_poke" => self.confirm_poke(arguments).await,
            "maimai_lxns_status" => self.status(arguments).await,
            "maimai_lxns_unbind" => self.unbind(arguments).await,
            _ => Err(
                ProtocolError::internal(format!("unreachable OAuth route: {name}"), None).into(),
            ),
        }
    }

    async fn authorization_url(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: OAuthUrlArgs = deserialize(arguments)?;
        let subject = subject(args.qq, args.subject)?;
        let state = trusted_state(args.state, &subject)?;
        let context = optional_context(
            args.adapter_id,
            args.adapter,
            args.group_id,
            args.conversation,
            args.bot_qq,
            args.bot,
        )?;
        let scopes = args
            .scopes
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let launch = self
            .service
            .authorization_url(
                subject,
                state,
                scopes,
                context,
                args.ttl_seconds.unwrap_or(600),
                now(),
            )
            .await?;
        let url = launch.url.to_string();
        let structured = authorization_dto(url.clone(), launch.expires_at)?;
        Ok(ToolOutput::text(format::authorization_url(&url)).with_structured_content(structured))
    }

    async fn bind_code(
        &self,
        arguments: Map<String, Value>,
        timeout: Option<Duration>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: BindCodeArgs = deserialize(arguments)?;
        let subject = subject(args.qq, args.subject)?;
        let state = trusted_state(args.state, &subject)?;
        let code = required_code(args.code)?;
        let context = optional_context(
            args.adapter_id,
            args.adapter,
            args.group_id,
            args.conversation,
            args.bot_qq,
            args.bot,
        )?;
        let result = match timeout {
            Some(timeout) => {
                self.service
                    .bind_code_with_timeout(subject, &code, state, context, now(), timeout)
                    .await?
            }
            None => {
                self.service
                    .bind_code(subject, &code, state, context, now())
                    .await?
            }
        };
        Ok(ToolOutput::text(format::bound()).with_structured_content(bound_dto(result)?))
    }

    async fn prepare_poke(
        &self,
        arguments: Map<String, Value>,
        timeout: Option<Duration>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: PreparePokeArgs = deserialize(arguments)?;
        let subject = subject(args.qq, args.subject)?;
        let state = trusted_state(args.state, &subject)?;
        let code = required_code(args.code)?;
        let context = required_context(
            args.adapter_id,
            args.adapter,
            args.group_id,
            args.conversation,
            args.bot_qq,
            args.bot,
        )?;
        let ttl_seconds = args.ttl_seconds.unwrap_or(300);
        let result = match timeout {
            Some(timeout) => {
                self.service
                    .prepare_poke_with_timeout(
                        subject,
                        &code,
                        state,
                        context,
                        PreparePokeTiming::new(ttl_seconds, now(), timeout),
                    )
                    .await?
            }
            None => {
                self.service
                    .prepare_poke(subject, &code, state, context, ttl_seconds, now())
                    .await?
            }
        };
        Ok(ToolOutput::text(format::pending()).with_structured_content(pending_dto(result)?))
    }

    async fn confirm_poke(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: ConfirmPokeArgs = deserialize(arguments)?;
        let subject = subject(args.qq, args.subject)?;
        let context = required_context(
            args.adapter_id,
            args.adapter,
            args.group_id,
            args.conversation,
            args.bot_qq,
            args.bot,
        )?;
        drop(args.state);
        let result = self.service.confirm_poke(subject, context, now()).await?;
        let dto = confirm_dto(result)?;
        let text = format::confirmed(&dto);
        Ok(ToolOutput::text(text).with_structured_content(confirm_value(&dto)))
    }

    async fn status(&self, arguments: Map<String, Value>) -> Result<ToolOutput, DispatchError> {
        let args: SubjectArgs = deserialize(arguments)?;
        let result = self
            .service
            .status(subject(args.qq, args.subject)?, now())
            .await?;
        let dto = status_dto(result)?;
        let text = format::status(&dto);
        Ok(ToolOutput::text(text).with_structured_content(to_value(dto)?))
    }

    async fn unbind(&self, arguments: Map<String, Value>) -> Result<ToolOutput, DispatchError> {
        let args: SubjectArgs = deserialize(arguments)?;
        let result = self.service.unbind(subject(args.qq, args.subject)?).await?;
        let dto = unbind_dto(result);
        let text = format::unbind(&dto);
        Ok(ToolOutput::text(text).with_structured_content(to_value(dto)?))
    }

    fn exchange_timeout(
        &self,
        arguments: &mut Map<String, Value>,
    ) -> Result<Option<Duration>, OAuthToolError> {
        if self.surface == OAuthSurface::Public {
            return Ok(None);
        }
        let seconds = match arguments.remove("timeout") {
            None | Some(Value::Null) => 30.0,
            Some(Value::Number(value)) => value.as_f64().ok_or_else(timeout_error)?,
            Some(_) => return Err(timeout_error()),
        };
        if !seconds.is_finite() || !(0.1..=300.0).contains(&seconds) {
            return Err(timeout_error());
        }
        Ok(Some(Duration::from_secs_f64(seconds)))
    }
}

fn timeout_error() -> OAuthToolError {
    OAuthToolError::invalid("timeout 必须是 0.1 到 300 秒之间的有限数。")
}

fn deserialize<T: DeserializeOwned>(arguments: Map<String, Value>) -> Result<T, OAuthToolError> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|_| OAuthToolError::invalid("工具参数格式不正确。"))
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
