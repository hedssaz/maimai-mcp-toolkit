use std::{sync::Arc, time::Duration};

use maimai_core::{ChartGeneration, Difficulty, GroupId, PlayerUsername, QqId, ScoreSource};
use maimai_render::{B50View, ChartType, Difficulty as RenderDifficulty, PlayerHeader, ScoreCard};
use thiserror::Error;
use time::OffsetDateTime;
use tokio::sync::Semaphore;

use crate::{
    identity::{IdentityDirectory, IdentityError, IdentityQuery, MaxResults},
    score_service::{
        B50Mode, B50Request, PlayerScoreService, PlayerScoreServiceError,
        PlayerScoreServiceErrorCode, ScoreQuery,
    },
    scores::{B50Chart, B50Result, Lookup, SourceSelection},
};

#[derive(Clone, Debug)]
pub enum B50ImageLookup {
    Qq(QqId),
    Username(PlayerUsername),
    Target(IdentityQuery),
}

pub struct B50ImageDataRequest {
    pub lookup: B50ImageLookup,
    pub group_id: Option<GroupId>,
    pub source: Option<ScoreSource>,
    pub title: Option<String>,
    pub timeout: Duration,
    pub now: OffsetDateTime,
}

pub struct B50ImageData {
    pub result: B50Result,
    pub selection: SourceSelection,
    pub view: B50View,
}

#[derive(Clone)]
pub struct B50ImageDataService {
    scores: Arc<PlayerScoreService>,
    identities: IdentityDirectory,
    query_slots: Arc<Semaphore>,
    max_query_concurrency: u32,
}

#[derive(Debug, Error)]
pub enum B50ImageDataError {
    #[error("QQ 身份缓存查询失败。")]
    Identity(#[source] IdentityError),
    #[error("target 匹配多个 QQ，请提供明确 QQ。")]
    AmbiguousIdentity { qqs: Vec<String> },
    #[error(transparent)]
    Score(#[from] PlayerScoreServiceError),
    #[error("B50 查询超时。")]
    Timeout,
    #[error("B50 查询任务异常终止。")]
    TaskJoin,
    #[error("B50 查询结果无法用于绘图。")]
    InvalidView,
}

impl B50ImageDataError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Identity(_) => "IDENTITY_ERROR",
            Self::AmbiguousIdentity { .. } => "AMBIGUOUS_IDENTITY",
            Self::Score(error) => match error.code() {
                PlayerScoreServiceErrorCode::InvalidLookup => "INVALID_LOOKUP",
                PlayerScoreServiceErrorCode::UnsupportedSource => "SOURCE_NOT_ALLOWED",
                PlayerScoreServiceErrorCode::SourceUnavailable => "SOURCE_EMPTY",
                PlayerScoreServiceErrorCode::AuthRequired => "AUTH_REQUIRED",
                PlayerScoreServiceErrorCode::Provider => "PROVIDER_ERROR",
                PlayerScoreServiceErrorCode::Storage => "STORAGE_ERROR",
                PlayerScoreServiceErrorCode::Catalog => "CATALOG_ERROR",
            },
            Self::Timeout => "TIMEOUT",
            Self::TaskJoin => "B50_QUERY_TASK_ERROR",
            Self::InvalidView => "INVALID_INPUT",
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Score(error) => error.status(),
            _ => None,
        }
    }

    pub fn ambiguous_qqs(&self) -> Option<&[String]> {
        match self {
            Self::AmbiguousIdentity { qqs } => Some(qqs),
            _ => None,
        }
    }
}

impl B50ImageDataService {
    pub fn new(scores: Arc<PlayerScoreService>, identities: IdentityDirectory) -> Self {
        Self::with_max_query_concurrency(scores, identities, 8)
    }

    pub fn with_max_query_concurrency(
        scores: Arc<PlayerScoreService>,
        identities: IdentityDirectory,
        max_query_concurrency: usize,
    ) -> Self {
        let max_query_concurrency = max_query_concurrency.max(1).min(u32::MAX as usize) as u32;
        Self {
            scores,
            identities,
            query_slots: Arc::new(Semaphore::new(max_query_concurrency as usize)),
            max_query_concurrency,
        }
    }

    pub async fn query(
        &self,
        request: B50ImageDataRequest,
    ) -> Result<B50ImageData, B50ImageDataError> {
        let started = tokio::time::Instant::now();
        let timeout = request.timeout;
        let permit = tokio::time::timeout(timeout, Arc::clone(&self.query_slots).acquire_owned())
            .await
            .map_err(|_| B50ImageDataError::Timeout)?
            .map_err(|_| B50ImageDataError::TaskJoin)?;
        let remaining = timeout
            .checked_sub(started.elapsed())
            .ok_or(B50ImageDataError::Timeout)?;
        let scores = Arc::clone(&self.scores);
        let identities = self.identities.clone();
        let task = tokio::spawn(async move {
            let _permit = permit;
            let lookup =
                resolve_lookup(&identities, request.lookup, request.group_id.as_ref()).await?;
            let response = scores
                .b50_with_evidence(
                    B50Request {
                        query: ScoreQuery {
                            lookup,
                            source: request.source,
                            diving_fish_credentials: None,
                            now: request.now.unix_timestamp(),
                        },
                        mode: B50Mode::Provider,
                    },
                    false,
                )
                .await?;
            let (result, _, selection) = response.into_parts();
            let view = view(&result, request.title)?;
            Ok(B50ImageData {
                result,
                selection,
                view,
            })
        });
        tokio::time::timeout(remaining, task)
            .await
            .map_err(|_| B50ImageDataError::Timeout)?
            .map_err(|_| B50ImageDataError::TaskJoin)?
    }

    /// Call after stopping new request ingress to let detached timed-out queries finish writes.
    /// Runtime shutdown remains the final cancellation boundary after this drain completes.
    pub async fn wait_idle(&self) -> Result<(), B50ImageDataError> {
        let permit = Arc::clone(&self.query_slots)
            .acquire_many_owned(self.max_query_concurrency)
            .await
            .map_err(|_| B50ImageDataError::TaskJoin)?;
        drop(permit);
        Ok(())
    }
}

async fn resolve_lookup(
    identities: &IdentityDirectory,
    lookup: B50ImageLookup,
    group_id: Option<&GroupId>,
) -> Result<Lookup, B50ImageDataError> {
    match lookup {
        B50ImageLookup::Qq(qq) => {
            identities
                .get_identity(&qq, group_id)
                .await
                .map_err(B50ImageDataError::Identity)?;
            Ok(Lookup::Qq(qq))
        }
        B50ImageLookup::Username(username) => Ok(Lookup::Username(username)),
        B50ImageLookup::Target(target) => {
            let resolution = identities
                .resolve_identity(&target, group_id, MaxResults::default_value())
                .await
                .map_err(B50ImageDataError::Identity)?;
            let mut matches = resolution.matches;
            if let Some(group_id) = group_id {
                matches.retain(|candidate| {
                    candidate
                        .identity
                        .groups
                        .iter()
                        .any(|group| &group.group_id == group_id)
                });
            }
            let Some(top_score) = matches.first().map(|value| value.score) else {
                let username = PlayerUsername::new(target.as_str().to_owned())
                    .map_err(|_| B50ImageDataError::InvalidView)?;
                return Ok(Lookup::Username(username));
            };
            let top_matches = matches
                .into_iter()
                .take_while(|value| value.score == top_score)
                .collect::<Vec<_>>();
            if top_matches.len() > 1 {
                let qqs = top_matches
                    .iter()
                    .map(|value| value.identity.qq.as_str().to_owned())
                    .collect();
                return Err(B50ImageDataError::AmbiguousIdentity { qqs });
            }
            if let Some(candidate) = top_matches.into_iter().next() {
                let qq = candidate.identity.qq.clone();
                return Ok(Lookup::Qq(qq));
            }
            let username = PlayerUsername::new(target.as_str().to_owned())
                .map_err(|_| B50ImageDataError::InvalidView)?;
            Ok(Lookup::Username(username))
        }
    }
}

fn view(result: &B50Result, title: Option<String>) -> Result<B50View, B50ImageDataError> {
    let nickname = result
        .player
        .nickname
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未知玩家");
    let rating = result
        .player
        .rating
        .or(result.player.actual_rating)
        .filter(|value| *value != 0);
    let player = PlayerHeader::new(nickname, rating, result.player.plate.clone())
        .map_err(|_| B50ImageDataError::InvalidView)?;
    let b35 = cards(&result.b35)?;
    let b15 = cards(&result.b15)?;
    B50View::new(
        title.unwrap_or_else(|| "maimai DX Best 50".to_owned()),
        player,
        result.rating_breakdown,
        b35,
        b15,
    )
    .map_err(|_| B50ImageDataError::InvalidView)
}

fn cards(charts: &[B50Chart]) -> Result<Vec<ScoreCard>, B50ImageDataError> {
    charts.iter().map(card).collect()
}

fn card(chart: &B50Chart) -> Result<ScoreCard, B50ImageDataError> {
    let chart_type = match chart.key.generation() {
        ChartGeneration::Standard => ChartType::Standard,
        ChartGeneration::Deluxe => ChartType::Deluxe,
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer => {
            return Err(B50ImageDataError::InvalidView);
        }
    };
    let difficulty = match chart.key.difficulty() {
        Difficulty::Basic => RenderDifficulty::Basic,
        Difficulty::Advanced => RenderDifficulty::Advanced,
        Difficulty::Expert => RenderDifficulty::Expert,
        Difficulty::Master => RenderDifficulty::Master,
        Difficulty::ReMaster => RenderDifficulty::ReMaster,
        Difficulty::Utage => return Err(B50ImageDataError::InvalidView),
    };
    ScoreCard::new(
        Some(chart.source_song_id.clone()),
        chart.title.clone(),
        chart_type,
        difficulty,
        chart.level.clone(),
        chart.constant,
        chart.achievements.and_then(|value| value.ranked()),
        chart.rating.unwrap_or_default(),
    )
    .and_then(|card| {
        card.with_markers(
            chart.grade.clone(),
            chart.full_combo.map(|value| value.as_str().to_owned()),
            chart.full_sync.map(|value| value.as_str().to_owned()),
        )
    })
    .map_err(|_| B50ImageDataError::InvalidView)
}
