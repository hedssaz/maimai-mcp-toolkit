mod convert;
mod dto;
mod error;
mod format;
#[cfg(test)]
mod tests;

use std::future::Future;

use maimai_app::identity::{IdentityQuery, IdentityService, MaxResults, ResetHour};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use time::{OffsetDateTime, UtcOffset};

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolOutput,
    contract::{ContractError, SurfaceContract},
};
use convert::{cache_dto, group_id, identity_dto, job_dto, qq, refresh_input, resolve_dto};
use dto::{EmptyArgs, GetArgs, RefreshArgs, ResolveArgs};
use error::IdentityToolError;

pub const MAIN_CONTRACT_JSON: &str = include_str!("../../../../contracts/main/identity.json");
pub const PUBLIC_CONTRACT_JSON: &str = include_str!("../../../../contracts/public/identity.json");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayOffset(UtcOffset);

impl DisplayOffset {
    pub const fn new(offset: UtcOffset) -> Self {
        Self(offset)
    }

    pub fn from_hours(hours: i8) -> Result<Self, time::error::ComponentRange> {
        UtcOffset::from_hms(hours, 0, 0).map(Self)
    }

    pub const fn get(self) -> UtcOffset {
        self.0
    }
}

#[derive(Clone)]
pub struct IdentityDispatcher {
    service: IdentityService,
    reset_hour: ResetHour,
    display_offset: DisplayOffset,
}

impl IdentityDispatcher {
    pub fn new(
        service: IdentityService,
        reset_hour: ResetHour,
    ) -> Result<Self, time::error::ComponentRange> {
        Ok(Self::with_display_offset(
            service,
            reset_hour,
            DisplayOffset::from_hours(8)?,
        ))
    }

    pub fn with_display_offset(
        service: IdentityService,
        reset_hour: ResetHour,
        display_offset: DisplayOffset,
    ) -> Self {
        Self {
            service,
            reset_hour,
            display_offset,
        }
    }
}

impl ToolDispatcher for IdentityDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move { dispatcher.dispatch_call(call).await }
    }
}

pub fn identity_server(
    contract_json: &str,
    dispatcher: IdentityDispatcher,
) -> Result<ContractServer<IdentityDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?;
    Ok(ContractServer::new(contract, dispatcher))
}

impl IdentityDispatcher {
    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        let name = call.name().to_owned();
        let arguments = call.into_arguments();
        match name.as_str() {
            "refresh_qq_identity_cache" => self.refresh(arguments).await,
            "qq_identity_cache_status" => self.cache_status(arguments).await,
            "qq_identity_job_status" => self.job_status(arguments).await,
            "resolve_qq_identity" => self.resolve(arguments).await,
            "get_qq_identity" => self.get(arguments).await,
            _ => Err(
                ProtocolError::internal(format!("unreachable identity route: {name}"), None).into(),
            ),
        }
    }

    async fn refresh(&self, arguments: Map<String, Value>) -> Result<ToolOutput, DispatchError> {
        let args: RefreshArgs = deserialize(arguments)?;
        let input = refresh_input(args, self.service.napcat_config())?;
        let now = OffsetDateTime::now_utc();
        let launch = match input.client {
            Some(client) => {
                self.service
                    .start_refresh_job_with_client(client, input.request, now, self.reset_hour)
                    .await?
            }
            None => {
                self.service
                    .start_refresh_job(input.request, now, self.reset_hour)
                    .await?
            }
        };
        let cache = cache_dto(&launch.cache, launch.job.as_ref())?;
        let job = launch.job.as_ref().map(job_dto).transpose()?;
        let text = if launch.started {
            format::job_started(
                job.as_ref().ok_or_else(IdentityToolError::internal)?,
                self.display_offset,
            )?
        } else {
            format::refresh_not_started(&cache, job.as_ref(), self.display_offset)?
        };
        let structured = json!({
            "started": launch.started,
            "cache": cache,
            "job": job,
            "text": text,
        });
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }

    async fn cache_status(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let _: EmptyArgs = deserialize(arguments)?;
        let now = OffsetDateTime::now_utc();
        let cache = self.service.cache_status(now, self.reset_hour).await?;
        let job = self.service.identity_job_status(now).await?;
        let dto = cache_dto(&cache, job.as_ref())?;
        let text = format::cache_status(&dto, None, self.display_offset)?;
        Ok(ToolOutput::text(text).with_structured_content(to_value(dto)?))
    }

    async fn job_status(&self, arguments: Map<String, Value>) -> Result<ToolOutput, DispatchError> {
        let _: EmptyArgs = deserialize(arguments)?;
        let now = OffsetDateTime::now_utc();
        let job = self.service.identity_job_status(now).await?;
        let cache = self.service.cache_status(now, self.reset_hour).await?;
        let job_dto = job.as_ref().map(job_dto).transpose()?;
        let cache_dto = cache_dto(&cache, job.as_ref())?;
        let text = format::job_status(job_dto.as_ref(), self.display_offset)?;
        let structured = json!({"job": job_dto, "cache": cache_dto, "text": text});
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }

    async fn resolve(&self, arguments: Map<String, Value>) -> Result<ToolOutput, DispatchError> {
        let args: ResolveArgs = deserialize(arguments)?;
        let now = OffsetDateTime::now_utc();
        self.service.initialize_identity_jobs(now).await?;
        let query = IdentityQuery::new(args.query)
            .map_err(|_| IdentityToolError::invalid("必须提供 query。"))?;
        let group = group_id(args.group_id)?;
        let limit = args.max_results.map_or_else(
            || Ok(MaxResults::default_value()),
            |value| {
                MaxResults::new(value).map_err(|_| {
                    IdentityToolError::invalid("maxResults 必须是 1 到 20 之间的整数。")
                })
            },
        )?;
        let resolution = self
            .service
            .resolve_identity(&query, group.as_ref(), limit)
            .await?;
        let cache = self.service.cache_status(now, self.reset_hour).await?;
        let dto = resolve_dto(resolution, &cache)?;
        let text = format::resolve(&dto);
        Ok(ToolOutput::text(text).with_structured_content(to_value(dto)?))
    }

    async fn get(&self, arguments: Map<String, Value>) -> Result<ToolOutput, DispatchError> {
        let args: GetArgs = deserialize(arguments)?;
        self.service
            .initialize_identity_jobs(OffsetDateTime::now_utc())
            .await?;
        let qq = qq(args.qq)?;
        let group = group_id(args.group_id)?;
        let identity = self
            .service
            .get_identity(&qq, group.as_ref())
            .await?
            .map(identity_dto);
        let text = format::identity(identity.as_ref(), qq.as_str());
        Ok(ToolOutput::text(text).with_structured_content(json!({"identity": identity})))
    }
}

fn deserialize<T: DeserializeOwned>(arguments: Map<String, Value>) -> Result<T, IdentityToolError> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|_| IdentityToolError::invalid("工具参数格式不正确。"))
}

fn to_value(value: impl serde::Serialize) -> Result<Value, IdentityToolError> {
    serde_json::to_value(value).map_err(|_| IdentityToolError::internal())
}
