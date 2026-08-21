mod convert;
mod dto;
mod error;
mod format;
mod render_data;
mod request;

use std::{future::Future, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use maimai_app::b50_image::{B50ImageDataService, B50ImageService, RenderOptions, utc_timestamp};
use maimai_core::SongIdValue;
use maimai_render::{MissingCover, SourceSongId};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use tokio::sync::Semaphore;

use crate::{
    ContractServer, DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolOutput,
    contract::{ContractError, SurfaceContract},
};
use dto::{EmptyArgs, RenderArgs, SetStyleArgs};
use error::B50ToolError;
use request::RenderInput;

pub const CONTRACT_JSON: &str = include_str!("../../../../contracts/main/b50_image.json");
const TOOLS: [&str; 3] = [
    "render_b50_image",
    "get_b50_image_style",
    "set_b50_image_style",
];

#[derive(Clone)]
pub struct B50ImageDispatcher {
    service: Arc<B50ImageService>,
    data: Arc<B50ImageDataService>,
    render_slots: Arc<Semaphore>,
}

impl B50ImageDispatcher {
    pub fn new(service: B50ImageService, data: Arc<B50ImageDataService>) -> Self {
        Self::with_shared_service_and_concurrency(Arc::new(service), data, 2)
    }

    pub fn with_max_render_concurrency(
        service: B50ImageService,
        data: Arc<B50ImageDataService>,
        max_render_concurrency: usize,
    ) -> Self {
        Self::with_shared_service_and_concurrency(Arc::new(service), data, max_render_concurrency)
    }

    pub fn with_shared_service(
        service: Arc<B50ImageService>,
        data: Arc<B50ImageDataService>,
    ) -> Self {
        Self::with_shared_service_and_concurrency(service, data, 2)
    }

    fn with_shared_service_and_concurrency(
        service: Arc<B50ImageService>,
        data: Arc<B50ImageDataService>,
        max_render_concurrency: usize,
    ) -> Self {
        Self {
            service,
            data,
            render_slots: Arc::new(Semaphore::new(max_render_concurrency.max(1))),
        }
    }

    async fn dispatch_call_at(
        &self,
        call: ToolCall,
        now: OffsetDateTime,
    ) -> Result<ToolOutput, DispatchError> {
        let name = call.name().to_owned();
        let arguments = call.into_arguments();
        match name.as_str() {
            "render_b50_image" => self.render(arguments, now).await,
            "get_b50_image_style" => self.get_style(arguments).await,
            "set_b50_image_style" => self.set_style(arguments, now).await,
            _ => Err(
                ProtocolError::internal(format!("unreachable B50 image route: {name}"), None)
                    .into(),
            ),
        }
    }

    async fn render(
        &self,
        arguments: Map<String, Value>,
        now: OffsetDateTime,
    ) -> Result<ToolOutput, DispatchError> {
        let started = tokio::time::Instant::now();
        let args: RenderArgs = deserialize(arguments)?;
        let prepared = request::input(&args, now)?;
        let data = match prepared.input {
            RenderInput::Provided => {
                let provided = args.b50_data.as_ref().ok_or(B50ToolError::Internal)?;
                render_data::provided(provided, args.title.clone())?
            }
            RenderInput::Query(request) => {
                let queried = self.data.query(request).await.map_err(B50ToolError::from)?;
                render_data::queried(queried, now)
            }
        };
        let output_mode = args.output_mode.unwrap_or_default();
        let options = RenderOptions {
            style: args.style,
            output_dir: request::optional_path(args.output_dir, "outputDir")?,
            static_dir: request::optional_path(args.static_dir, "staticDir")?,
            cover_cache_dir: request::optional_path(args.cover_cache_dir, "coverCacheDir")?,
            filename_stem: data.filename_stem,
        };
        let remaining = prepared
            .timeout
            .checked_sub(started.elapsed())
            .ok_or(B50ToolError::Timeout)?;
        let permit =
            tokio::time::timeout(remaining, Arc::clone(&self.render_slots).acquire_owned())
                .await
                .map_err(|_| B50ToolError::Timeout)?
                .map_err(|_| B50ToolError::Internal)?;
        let service = Arc::clone(&self.service);
        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            service.render(&data.view, options, now)
        })
        .await
        .map_err(|_| B50ToolError::Internal)?
        .map_err(B50ToolError::from)?;
        let missing_covers = result
            .rendered
            .metadata
            .missing_covers
            .iter()
            .map(missing_cover)
            .collect::<Vec<_>>();
        let text = format::render_text(
            data.nickname.as_deref(),
            data.rating,
            &result.image_path,
            missing_covers.len(),
        );
        let mut structured = json!({
            "imagePath": result.image_path,
            "mimeType": "image/png",
            "width": result.rendered.metadata.width,
            "height": result.rendered.metadata.height,
            "outputMode": output_mode.as_str(),
            "lookup": data.lookup,
            "player": data.player,
            "counts": data.counts,
            "ratingBreakdown": data.rating_breakdown,
            "style": result.style,
            "missingCovers": missing_covers,
            "generatedAt": utc_timestamp(now),
            "caption": data.caption,
            "text": text,
        });
        if output_mode.includes_base64()
            && let Some(object) = structured.as_object_mut()
        {
            object.insert(
                "imageBase64".to_owned(),
                json!(STANDARD.encode(&result.rendered.bytes)),
            );
        }
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }

    async fn get_style(&self, arguments: Map<String, Value>) -> Result<ToolOutput, DispatchError> {
        let _: EmptyArgs = deserialize(arguments)?;
        let service = Arc::clone(&self.service);
        let selection = tokio::task::spawn_blocking(move || service.current_style())
            .await
            .map_err(|_| B50ToolError::Internal)?
            .map_err(B50ToolError::from)?;
        let text = format!("当前 B50 图片默认风格: {}", selection.style);
        let structured = json!({
            "style": selection.style,
            "text": text,
        });
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }

    async fn set_style(
        &self,
        arguments: Map<String, Value>,
        now: OffsetDateTime,
    ) -> Result<ToolOutput, DispatchError> {
        let args: SetStyleArgs = deserialize(arguments)?;
        let service = Arc::clone(&self.service);
        let update = tokio::task::spawn_blocking(move || service.set_style(args.style, now))
            .await
            .map_err(|_| B50ToolError::Internal)?
            .map_err(B50ToolError::from)?;
        let text = format!("B50 图片默认风格已切换为: {}", update.style);
        let structured = json!({
            "style": update.style,
            "updatedAt": update.updated_at,
            "text": text,
        });
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }
}

impl ToolDispatcher for B50ImageDispatcher {
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

pub fn b50_image_server(
    dispatcher: B50ImageDispatcher,
) -> Result<ContractServer<B50ImageDispatcher>, ContractError> {
    let contract = SurfaceContract::parse(CONTRACT_JSON)?.retain_tools(&TOOLS);
    Ok(ContractServer::new(contract, dispatcher))
}

fn missing_cover(value: &MissingCover) -> Value {
    json!({
        "songId": value.song_id.as_ref().map(song_id_value),
        "title": value.title,
    })
}

fn song_id_value(value: &SourceSongId) -> Value {
    match value.value() {
        SongIdValue::Numeric(value) => json!(value),
        SongIdValue::Text(value) => json!(value.as_str()),
    }
}

fn deserialize<T: DeserializeOwned>(arguments: Map<String, Value>) -> Result<T, B50ToolError> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|_| B50ToolError::invalid("工具参数格式不正确。"))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod provider_tests;
