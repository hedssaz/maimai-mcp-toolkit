use reqwest::Method;
use serde::Deserialize;

use crate::{ProviderEnvelope, RawJsonPayload};

use super::{
    LxnsPlayer, LxnsPlayerBests, LxnsPlayerScores, LxnsScore, LxnsScoreConfig, LxnsScoreEndpoint,
    LxnsScoreError, LxnsScoreErrorCode, LxnsSongBests, LxnsSongId, PlayerUpdate, ScoreUpload,
    UploadReceipt,
    request::{RequestBody, ResponsePayload, send},
};

#[derive(Debug)]
pub struct LxnsScoreClient {
    endpoint: LxnsScoreEndpoint,
    config: LxnsScoreConfig,
}

impl LxnsScoreClient {
    pub fn new(config: LxnsScoreConfig) -> Result<Self, LxnsScoreError> {
        let (base_url, timeout, access_token) = config.into_parts();
        LxnsScoreEndpoint::new(base_url, timeout)?.authorize(access_token)
    }

    pub(super) fn authorized(endpoint: LxnsScoreEndpoint, config: LxnsScoreConfig) -> Self {
        Self { endpoint, config }
    }

    pub fn config(&self) -> &LxnsScoreConfig {
        &self.config
    }

    pub async fn player(&self) -> Result<LxnsPlayer, LxnsScoreError> {
        let payload = self.get("user/maimai/player", None).await?;
        serde_json::from_value(payload.value)
            .map_err(|_| shape_error("player", payload.status, payload.sanitized_body))
    }

    pub async fn scores(&self) -> Result<LxnsPlayerScores, LxnsScoreError> {
        let payload = self.get("user/maimai/player/scores", None).await?;
        decode_scores(payload)
    }

    pub async fn scores_with_raw(
        &self,
    ) -> Result<ProviderEnvelope<LxnsPlayerScores>, LxnsScoreError> {
        let payload = self.get("user/maimai/player/scores", None).await?;
        let raw = raw_payload(&payload.value)?;
        Ok(ProviderEnvelope::new(decode_scores(payload)?, raw))
    }

    pub async fn bests(&self) -> Result<LxnsPlayerBests, LxnsScoreError> {
        let payload = self.get("user/maimai/player/bests", None).await?;
        decode_bests(payload)
    }

    pub async fn bests_with_raw(
        &self,
    ) -> Result<ProviderEnvelope<LxnsPlayerBests>, LxnsScoreError> {
        let payload = self.get("user/maimai/player/bests", None).await?;
        let raw = raw_payload(&payload.value)?;
        Ok(ProviderEnvelope::new(decode_bests(payload)?, raw))
    }

    pub async fn song_bests(&self, song_id: LxnsSongId) -> Result<LxnsSongBests, LxnsScoreError> {
        let (payload, query_id) = self.song_payload(song_id).await?;
        decode_song(payload, query_id)
    }

    pub async fn song_bests_with_raw(
        &self,
        song_id: LxnsSongId,
    ) -> Result<ProviderEnvelope<LxnsSongBests>, LxnsScoreError> {
        let (payload, query_id) = self.song_payload(song_id).await?;
        let raw = raw_payload(&payload.value)?;
        Ok(ProviderEnvelope::new(decode_song(payload, query_id)?, raw))
    }

    async fn song_payload(
        &self,
        song_id: LxnsSongId,
    ) -> Result<(ResponsePayload, LxnsSongId), LxnsScoreError> {
        let query_id = LxnsSongId::for_query(song_id.get())?;
        let payload = self
            .get(
                "user/maimai/player/bests",
                Some(("song_id", query_id.get())),
            )
            .await?;
        Ok((payload, query_id))
    }

    /*
     * Write operations remain on this authorized client; rotating access tokens only
     * replaces the lightweight client handle and keeps the endpoint connection pool.
     */

    pub async fn update_player(&self, update: &PlayerUpdate) -> Result<LxnsPlayer, LxnsScoreError> {
        let payload = send(
            self.endpoint.http(),
            &self.config,
            Method::PUT,
            "user/maimai/player",
            None,
            Some(RequestBody::Player(update)),
        )
        .await?;
        if !payload.value.is_object() {
            return Ok(LxnsPlayer::from(update));
        }
        serde_json::from_value(payload.value)
            .map_err(|_| shape_error("updated player", payload.status, payload.sanitized_body))
    }

    pub async fn upload_scores(
        &self,
        scores: &[ScoreUpload],
    ) -> Result<UploadReceipt, LxnsScoreError> {
        if scores.is_empty() {
            return Err(LxnsScoreError::new(
                LxnsScoreErrorCode::InvalidRequest,
                "LXNS score upload 不能为空",
            ));
        }
        let payload = send(
            self.endpoint.http(),
            &self.config,
            Method::POST,
            "user/maimai/player/scores",
            None,
            Some(RequestBody::Scores(scores)),
        )
        .await?;
        let updated = if !payload.value.is_object() {
            None
        } else {
            serde_json::from_value::<UploadData>(payload.value)
                .map_err(|_| shape_error("score upload", payload.status, payload.sanitized_body))?
                .updated
        };
        Ok(UploadReceipt {
            uploaded: scores.len(),
            updated,
        })
    }

    async fn get(
        &self,
        path: &str,
        query: Option<(&str, u32)>,
    ) -> Result<ResponsePayload, LxnsScoreError> {
        send(
            self.endpoint.http(),
            &self.config,
            Method::GET,
            path,
            query,
            None,
        )
        .await
    }
}

fn decode_scores(payload: ResponsePayload) -> Result<LxnsPlayerScores, LxnsScoreError> {
    match serde_json::from_value::<ScoresPayload>(payload.value)
        .map_err(|_| shape_error("scores", payload.status, payload.sanitized_body))?
    {
        ScoresPayload::List(scores) => Ok(LxnsPlayerScores {
            player: None,
            scores,
        }),
        ScoresPayload::Object(object) => Ok(LxnsPlayerScores {
            player: object.player,
            scores: object.scores,
        }),
    }
}

fn decode_bests(payload: ResponsePayload) -> Result<LxnsPlayerBests, LxnsScoreError> {
    serde_json::from_value::<BestsObject>(payload.value)
        .map(BestsObject::into_bests)
        .map_err(|_| shape_error("bests", payload.status, payload.sanitized_body))
}

fn decode_song(
    payload: ResponsePayload,
    query_id: LxnsSongId,
) -> Result<LxnsSongBests, LxnsScoreError> {
    let (scores, player) = match serde_json::from_value::<SongPayload>(payload.value)
        .map_err(|_| shape_error("song bests", payload.status, payload.sanitized_body))?
    {
        SongPayload::List(scores) => (scores, None),
        SongPayload::Single(score) => (vec![score], None),
        SongPayload::Object(object) => object.into_scores(),
    };
    Ok(LxnsSongBests {
        song_id: query_id,
        player,
        scores,
    })
}

fn raw_payload(value: &serde_json::Value) -> Result<RawJsonPayload, LxnsScoreError> {
    RawJsonPayload::from_value(value).map_err(|_| {
        LxnsScoreError::new(
            LxnsScoreErrorCode::ResponseTooLarge,
            "LXNS score 原始响应超过安全上限",
        )
    })
}

impl From<&PlayerUpdate> for LxnsPlayer {
    fn from(value: &PlayerUpdate) -> Self {
        Self {
            name: Some(value.name.clone()),
            friend_code: Some(value.friend_code.clone()),
            rating: Some(value.rating),
            course_rank: Some(value.course_rank),
            class_rank: Some(value.class_rank),
            star: Some(value.star),
            trophy: value.trophy.clone(),
            icon: value.icon.clone(),
            name_plate: value.name_plate.clone(),
            frame: value.frame.clone(),
            upload_time: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ScoresPayload {
    List(Vec<LxnsScore>),
    Object(Box<ScoresObject>),
}

#[derive(Deserialize)]
struct ScoresObject {
    #[serde(default)]
    player: Option<LxnsPlayer>,
    #[serde(default)]
    scores: Vec<LxnsScore>,
}

#[derive(Deserialize)]
struct BestsObject {
    #[serde(default)]
    player: Option<LxnsPlayer>,
    #[serde(default)]
    standard: Vec<LxnsScore>,
    #[serde(default)]
    sd: Vec<LxnsScore>,
    #[serde(default)]
    b35: Vec<LxnsScore>,
    #[serde(default)]
    dx: Vec<LxnsScore>,
    #[serde(default)]
    new: Vec<LxnsScore>,
    #[serde(default)]
    b15: Vec<LxnsScore>,
    #[serde(default)]
    standard_total: Option<u32>,
    #[serde(default, rename = "standardTotal")]
    standard_total_camel: Option<u32>,
    #[serde(default)]
    sd_total: Option<u32>,
    #[serde(default)]
    b35_total: Option<u32>,
    #[serde(default, rename = "b35Total")]
    b35_total_camel: Option<u32>,
    #[serde(default)]
    dx_total: Option<u32>,
    #[serde(default, rename = "dxTotal")]
    dx_total_camel: Option<u32>,
    #[serde(default)]
    new_total: Option<u32>,
    #[serde(default)]
    b15_total: Option<u32>,
    #[serde(default, rename = "b15Total")]
    b15_total_camel: Option<u32>,
}

impl BestsObject {
    fn into_bests(self) -> LxnsPlayerBests {
        LxnsPlayerBests {
            player: self.player,
            standard: first_nonempty([self.standard, self.sd, self.b35]),
            deluxe: first_nonempty([self.dx, self.new, self.b15]),
            standard_total: first_positive([
                self.standard_total,
                self.standard_total_camel,
                self.sd_total,
                self.b35_total,
                self.b35_total_camel,
            ]),
            deluxe_total: first_positive([
                self.dx_total,
                self.dx_total_camel,
                self.new_total,
                self.b15_total,
                self.b15_total_camel,
            ]),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SongPayload {
    List(Vec<LxnsScore>),
    Single(LxnsScore),
    Object(Box<SongObject>),
}

#[derive(Default, Deserialize)]
struct SongObject {
    #[serde(default)]
    player: Option<LxnsPlayer>,
    #[serde(default)]
    scores: Vec<LxnsScore>,
    #[serde(default)]
    bests: Vec<LxnsScore>,
    #[serde(default)]
    standard: Vec<LxnsScore>,
    #[serde(default)]
    sd: Vec<LxnsScore>,
    #[serde(default)]
    dx: Vec<LxnsScore>,
    #[serde(default)]
    new: Vec<LxnsScore>,
    #[serde(default)]
    b35: Vec<LxnsScore>,
    #[serde(default)]
    b15: Vec<LxnsScore>,
}

impl SongObject {
    fn into_scores(self) -> (Vec<LxnsScore>, Option<LxnsPlayer>) {
        let mut scores = self.scores;
        scores.extend(self.bests);
        scores.extend(self.standard);
        scores.extend(self.sd);
        scores.extend(self.dx);
        scores.extend(self.new);
        scores.extend(self.b35);
        scores.extend(self.b15);
        (scores, self.player)
    }
}

#[derive(Deserialize)]
struct UploadData {
    #[serde(default)]
    updated: Option<u64>,
}

fn first_nonempty<const N: usize>(values: [Vec<LxnsScore>; N]) -> Vec<LxnsScore> {
    values
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn first_positive<const N: usize>(values: [Option<u32>; N]) -> Option<u32> {
    values.into_iter().flatten().find(|value| *value > 0)
}

fn shape_error(label: &str, status: u16, body: String) -> LxnsScoreError {
    LxnsScoreError::response(
        LxnsScoreErrorCode::InvalidShape,
        format!("LXNS {label} 响应结构不正确"),
        status,
        Some(body),
    )
}
