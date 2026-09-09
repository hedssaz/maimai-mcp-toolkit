use maimai_providers::lxns_score::{
    LxnsPlayer, LxnsPlayerBests, LxnsPlayerScores, LxnsScoreEndpoint, LxnsScoreError,
    LxnsScoreErrorCode, LxnsSongBests, LxnsSongId,
};
use maimai_providers::{LxnsScoreClient, RawJsonPayload};

use crate::oauth::{OAuthService, OAuthSubject};

use super::PlayerScoreServiceError;

pub(super) async fn player(
    oauth: &OAuthService,
    endpoint: &LxnsScoreEndpoint,
    subject: OAuthSubject,
    now: i64,
) -> Result<LxnsPlayer, PlayerScoreServiceError> {
    let grant = oauth
        .access_token(subject.clone(), now)
        .await
        .map_err(PlayerScoreServiceError::oauth)?;
    let client = endpoint
        .authorize(grant.access_token().clone())
        .map_err(PlayerScoreServiceError::lxns)?;
    match client.player().await {
        Err(error) if error.code() == LxnsScoreErrorCode::Unauthorized => {
            let grant = oauth
                .retry_after_unauthorized(subject, grant.generation(), now)
                .await
                .map_err(PlayerScoreServiceError::oauth)?;
            endpoint
                .authorize(grant.access_token().clone())
                .map_err(PlayerScoreServiceError::lxns)?
                .player()
                .await
                .map_err(PlayerScoreServiceError::lxns)
        }
        result => result.map_err(PlayerScoreServiceError::lxns),
    }
}

pub(super) async fn bests(
    oauth: &OAuthService,
    endpoint: &LxnsScoreEndpoint,
    subject: OAuthSubject,
    now: i64,
    include_raw: bool,
) -> Result<(LxnsPlayerBests, Option<RawJsonPayload>), PlayerScoreServiceError> {
    let grant = oauth
        .access_token(subject.clone(), now)
        .await
        .map_err(PlayerScoreServiceError::oauth)?;
    let client = endpoint
        .authorize(grant.access_token().clone())
        .map_err(PlayerScoreServiceError::lxns)?;
    match request_bests(&client, include_raw).await {
        Err(error) if error.code() == LxnsScoreErrorCode::Unauthorized => {
            let grant = oauth
                .retry_after_unauthorized(subject, grant.generation(), now)
                .await
                .map_err(PlayerScoreServiceError::oauth)?;
            let client = endpoint
                .authorize(grant.access_token().clone())
                .map_err(PlayerScoreServiceError::lxns)?;
            request_bests(&client, include_raw)
                .await
                .map_err(PlayerScoreServiceError::lxns)
        }
        result => result.map_err(PlayerScoreServiceError::lxns),
    }
}

pub(super) async fn scores(
    oauth: &OAuthService,
    endpoint: &LxnsScoreEndpoint,
    subject: OAuthSubject,
    now: i64,
    include_raw: bool,
) -> Result<(LxnsPlayerScores, Option<RawJsonPayload>), PlayerScoreServiceError> {
    let grant = oauth
        .access_token(subject.clone(), now)
        .await
        .map_err(PlayerScoreServiceError::oauth)?;
    let client = endpoint
        .authorize(grant.access_token().clone())
        .map_err(PlayerScoreServiceError::lxns)?;
    match request_scores(&client, include_raw).await {
        Err(error) if error.code() == LxnsScoreErrorCode::Unauthorized => {
            let grant = oauth
                .retry_after_unauthorized(subject, grant.generation(), now)
                .await
                .map_err(PlayerScoreServiceError::oauth)?;
            let client = endpoint
                .authorize(grant.access_token().clone())
                .map_err(PlayerScoreServiceError::lxns)?;
            request_scores(&client, include_raw)
                .await
                .map_err(PlayerScoreServiceError::lxns)
        }
        result => result.map_err(PlayerScoreServiceError::lxns),
    }
}

pub(super) async fn song_bests(
    oauth: &OAuthService,
    endpoint: &LxnsScoreEndpoint,
    subject: OAuthSubject,
    song_id: LxnsSongId,
    now: i64,
    include_raw: bool,
) -> Result<(LxnsSongBests, Option<RawJsonPayload>), PlayerScoreServiceError> {
    let grant = oauth
        .access_token(subject.clone(), now)
        .await
        .map_err(PlayerScoreServiceError::oauth)?;
    let client = endpoint
        .authorize(grant.access_token().clone())
        .map_err(PlayerScoreServiceError::lxns)?;
    match request_song(&client, song_id, include_raw).await {
        Err(error) if error.code() == LxnsScoreErrorCode::Unauthorized => {
            let grant = oauth
                .retry_after_unauthorized(subject, grant.generation(), now)
                .await
                .map_err(PlayerScoreServiceError::oauth)?;
            let client = endpoint
                .authorize(grant.access_token().clone())
                .map_err(PlayerScoreServiceError::lxns)?;
            request_song(&client, song_id, include_raw)
                .await
                .map_err(PlayerScoreServiceError::lxns)
        }
        result => result.map_err(PlayerScoreServiceError::lxns),
    }
}

async fn request_bests(
    client: &LxnsScoreClient,
    include_raw: bool,
) -> Result<(LxnsPlayerBests, Option<RawJsonPayload>), LxnsScoreError> {
    let (mut bests, raw) = if include_raw {
        let (data, raw) = client.bests_with_raw().await?.into_parts();
        (data, Some(raw))
    } else {
        (client.bests().await?, None)
    };
    if bests.player.is_none() {
        bests.player = Some(client.player().await?);
    }
    Ok((bests, raw))
}

async fn request_scores(
    client: &LxnsScoreClient,
    include_raw: bool,
) -> Result<(LxnsPlayerScores, Option<RawJsonPayload>), LxnsScoreError> {
    if include_raw {
        let (data, raw) = client.scores_with_raw().await?.into_parts();
        Ok((data, Some(raw)))
    } else {
        client.scores().await.map(|data| (data, None))
    }
}

async fn request_song(
    client: &LxnsScoreClient,
    song_id: LxnsSongId,
    include_raw: bool,
) -> Result<(LxnsSongBests, Option<RawJsonPayload>), LxnsScoreError> {
    if include_raw {
        let (data, raw) = client.song_bests_with_raw(song_id).await?.into_parts();
        Ok((data, Some(raw)))
    } else {
        client.song_bests(song_id).await.map(|data| (data, None))
    }
}
