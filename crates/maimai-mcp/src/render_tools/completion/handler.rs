use std::{future::Future, sync::Arc};

use maimai_app::completion::CompletionService;
use time::OffsetDateTime;

use crate::{DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure, ToolOutput};

use super::{
    CompletionSurface, CompletionToolError, convert,
    dto::{PlateArgs, PlateBatchArgs, ProgressArgs, RatingArgs},
    format,
};

#[derive(Clone)]
pub struct CompletionDispatcher {
    service: Arc<CompletionService>,
    surface: CompletionSurface,
}

impl CompletionDispatcher {
    pub fn new(
        service: Arc<CompletionService>,
        surface: CompletionSurface,
    ) -> Result<Self, CompletionToolError> {
        if service.capabilities().is_public() != (surface == CompletionSurface::Public) {
            return Err(CompletionToolError::invalid(
                "completion service capability profile 与 MCP surface 不一致",
            ));
        }
        Ok(Self { service, surface })
    }

    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        match call.name() {
            "render_maimai_plate" => {
                let args: PlateArgs = call.deserialize()?;
                let (identity, spec) = convert::plate(args, self.surface).map_err(tool_error)?;
                let stem = stem(&identity);
                let result = self
                    .service
                    .render_plate(identity, spec, stem, OffsetDateTime::now_utc())
                    .await
                    .map_err(|error| prefixed("渲染牌子表失败", error))?;
                Ok(ToolOutput::text(
                    format::image(&result, self.surface).map_err(tool_error)?,
                ))
            }
            "render_maimai_plate_batch" => {
                let args: PlateBatchArgs = call.deserialize()?;
                let (identity, specs) =
                    convert::plate_batch(args, self.surface).map_err(tool_error)?;
                let stem = stem(&identity);
                let result = self
                    .service
                    .render_plate_batch(identity, specs, stem, OffsetDateTime::now_utc())
                    .await
                    .map_err(|error| prefixed("批量渲染牌子表失败", error))?;
                let text = format::plate_batch(&result, self.surface).map_err(tool_error)?;
                if result.results.is_empty() && !result.errors.is_empty() {
                    Err(ToolFailure::text(text).into())
                } else {
                    Ok(ToolOutput::text(text))
                }
            }
            "render_maimai_progress" => {
                let args: ProgressArgs = call.deserialize()?;
                let request = convert::progress(args, self.surface).map_err(tool_error)?;
                let stem = stem(&request.identity);
                let result = self
                    .service
                    .render_level_progress(request, stem, OffsetDateTime::now_utc())
                    .await
                    .map_err(|error| prefixed("渲染进度图失败", error))?;
                Ok(ToolOutput::text(
                    format::image(&result, self.surface).map_err(tool_error)?,
                ))
            }
            "render_maimai_rating" => {
                let args: RatingArgs = call.deserialize()?;
                let request = convert::rating(args, self.surface).map_err(tool_error)?;
                let stem = stem(&request.identity);
                let result = self
                    .service
                    .render_rating_table(request, stem, OffsetDateTime::now_utc())
                    .await
                    .map_err(|error| prefixed("渲染定数表失败", error))?;
                Ok(ToolOutput::text(
                    format::rating_image(&result, self.surface).map_err(tool_error)?,
                ))
            }
            "render_maimai_plate_progress" => {
                let args: PlateArgs = call.deserialize()?;
                let (identity, spec) = convert::plate(args, self.surface).map_err(tool_error)?;
                let stem = stem(&identity);
                let result = self
                    .service
                    .plate_progress(identity, spec, stem, OffsetDateTime::now_utc())
                    .await
                    .map_err(|error| prefixed("查询牌子进度失败", error))?;
                Ok(ToolOutput::text(
                    format::plate_progress(&result, self.surface).map_err(tool_error)?,
                ))
            }
            "render_maimai_plate_progress_batch" => {
                let args: PlateBatchArgs = call.deserialize()?;
                let (identity, specs) =
                    convert::plate_batch(args, self.surface).map_err(tool_error)?;
                let stem = stem(&identity);
                let result = self
                    .service
                    .plate_progress_batch(identity, specs, stem, OffsetDateTime::now_utc())
                    .await
                    .map_err(|error| prefixed("批量查询牌子进度失败", error))?;
                let text =
                    format::plate_progress_batch(&result, self.surface).map_err(tool_error)?;
                if result.results.is_empty() && !result.errors.is_empty() {
                    Err(ToolFailure::text(text).into())
                } else {
                    Ok(ToolOutput::text(text))
                }
            }
            name => Err(ProtocolError::internal(
                format!("unreachable completion render route: {name}"),
                None,
            )
            .into()),
        }
    }
}

impl ToolDispatcher for CompletionDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move { dispatcher.dispatch_call(call).await }
    }
}

fn stem(identity: &maimai_app::completion::CompletionIdentity) -> String {
    match &identity.lookup {
        maimai_app::scores::Lookup::Qq(value) => value.as_str().to_owned(),
        maimai_app::scores::Lookup::Username(value) => value.as_str().to_owned(),
    }
}

fn tool_error(error: CompletionToolError) -> DispatchError {
    ToolFailure::text(error.to_string()).into()
}

fn prefixed(prefix: &str, error: maimai_app::completion::CompletionError) -> DispatchError {
    ToolFailure::text(format!("{prefix}: {error}")).into()
}
