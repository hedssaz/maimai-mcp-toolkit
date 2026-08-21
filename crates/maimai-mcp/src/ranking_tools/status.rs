use maimai_storage::RankingNamespace;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use super::{RankingDispatcher, convert, dto::CacheArgs, error::RankingToolError, format};
use crate::{DispatchError, ToolOutput};

impl RankingDispatcher {
    pub(super) async fn b50_cache_status(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        self.cache_status(arguments, RankingNamespace::B50).await
    }

    pub(super) async fn song_cache_status(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        self.cache_status(arguments, RankingNamespace::SongScore)
            .await
    }

    pub(super) async fn song_job_status(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        let args: CacheArgs = super::deserialize(arguments)?;
        let group = convert::group_id(args.group_id)?;
        let now = OffsetDateTime::now_utc();
        let job = self
            .service
            .job_status(RankingNamespace::SongScore, &group, now)
            .await
            .map_err(RankingToolError::from)?;
        let status = self
            .service
            .cache_status(RankingNamespace::SongScore, &group, now)
            .await
            .map_err(RankingToolError::from)?;
        super::output(
            format::job_status_text(job.as_ref(), &status, self.display_offset),
            json!({
                "groupId": group.as_str(), "feature": "song_score",
                "job": job.as_ref().map(format::job_value),
                "cache": format::cache_value(&status),
                "text": format::job_status_text(job.as_ref(), &status, self.display_offset),
                "data": if status.exists { format::cache_value(&status) } else { Value::Null },
            }),
        )
    }

    pub(super) async fn clear_b50(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        self.clear(arguments, RankingNamespace::B50).await
    }

    pub(super) async fn clear_song(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<ToolOutput, DispatchError> {
        self.clear(arguments, RankingNamespace::SongScore).await
    }

    async fn cache_status(
        &self,
        arguments: Map<String, Value>,
        namespace: RankingNamespace,
    ) -> Result<ToolOutput, DispatchError> {
        let args: CacheArgs = super::deserialize(arguments)?;
        let group = convert::group_id(args.group_id)?;
        let status = self
            .service
            .cache_status(namespace, &group, OffsetDateTime::now_utc())
            .await
            .map_err(RankingToolError::from)?;
        super::output(
            format::cache_status_text(&status, self.display_offset),
            format::cache_value(&status),
        )
    }

    async fn clear(
        &self,
        arguments: Map<String, Value>,
        namespace: RankingNamespace,
    ) -> Result<ToolOutput, DispatchError> {
        let args: CacheArgs = super::deserialize(arguments)?;
        let group = convert::group_id(args.group_id)?;
        let cleared = self
            .service
            .clear_cache(namespace, &group)
            .await
            .map_err(RankingToolError::from)?;
        let text = if cleared {
            format!("群 {} 缓存已清除。", group.as_str())
        } else {
            format!("群 {} 没有可清除的缓存。", group.as_str())
        };
        super::output(
            text,
            json!({
                "groupId": group.as_str(), "feature": namespace.as_str(), "cleared": cleared,
            }),
        )
    }
}
