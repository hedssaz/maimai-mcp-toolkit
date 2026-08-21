use std::{future::Future, sync::Arc};

use maimai_app::music_score::{MusicScoreError, MusicScoreService};
use time::OffsetDateTime;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure,
    ToolOutput,
    contract::{ContractError, SurfaceContract},
};

use super::{convert, dto::MusicScoreArgs, format};

pub const TOOL_NAME: &str = "render_maimai_music_score";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MusicScoreSurface {
    Main,
    Public,
}

#[derive(Clone)]
pub struct MusicScoreDispatcher {
    service: Arc<MusicScoreService>,
    surface: MusicScoreSurface,
}

impl MusicScoreDispatcher {
    pub fn main(service: Arc<MusicScoreService>) -> Self {
        Self {
            service,
            surface: MusicScoreSurface::Main,
        }
    }

    pub fn public(service: Arc<MusicScoreService>) -> Self {
        Self {
            service,
            surface: MusicScoreSurface::Public,
        }
    }

    async fn dispatch_call(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        if call.name() != TOOL_NAME {
            return Err(ProtocolError::internal(
                format!("unreachable music score route: {}", call.name()),
                None,
            )
            .into());
        }
        let args: MusicScoreArgs = call.deserialize()?;
        let request = convert::request(args, self.surface).map_err(tool_error)?;
        let result = self
            .service
            .render(request, OffsetDateTime::now_utc())
            .await
            .map_err(|error| ToolFailure::text(error_text(&error)))?;
        let text = format::result(&result, self.surface).map_err(tool_error)?;
        if result.images.is_empty() {
            Err(ToolFailure::text(text).into())
        } else {
            Ok(ToolOutput::text(text))
        }
    }
}

impl ToolDispatcher for MusicScoreDispatcher {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
        let dispatcher = self.clone();
        async move { dispatcher.dispatch_call(call).await }
    }
}

pub fn main_music_score_server(
    contract_json: &str,
    dispatcher: MusicScoreDispatcher,
) -> Result<ContractServer<MusicScoreDispatcher>, ContractError> {
    server(contract_json, dispatcher)
}

pub fn public_music_score_server(
    contract_json: &str,
    dispatcher: MusicScoreDispatcher,
) -> Result<ContractServer<MusicScoreDispatcher>, ContractError> {
    server(contract_json, dispatcher)
}

fn server(
    contract_json: &str,
    dispatcher: MusicScoreDispatcher,
) -> Result<ContractServer<MusicScoreDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(contract_json)?.retain_tools(&[TOOL_NAME]);
    Ok(ContractServer::new(contract, dispatcher))
}

fn error_text(error: &MusicScoreError) -> String {
    match error {
        MusicScoreError::MusicResolution(_) | MusicScoreError::Scores(_) => error.to_string(),
        MusicScoreError::TaskJoin | MusicScoreError::Render(_) | MusicScoreError::Output(_) => {
            format!("渲染单曲成绩图失败: {error}")
        }
    }
}

fn tool_error(error: crate::render_tools::RenderToolError) -> DispatchError {
    ToolFailure::text(error.to_string()).into()
}
