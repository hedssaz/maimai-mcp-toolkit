use maimai_providers::OAuthErrorCode;
use maimai_storage::{OAuthCasResult, OAuthTokenRecord};

use super::{
    AccessGrant, OAuthService, OAuthServiceError, OAuthServiceErrorCode, OAuthSubject,
    tokens::{access_grant, new_stored_token},
};

const REFRESH_SKEW_SECONDS: i64 = 90;

impl OAuthService {
    pub async fn access_token(
        &self,
        subject: OAuthSubject,
        now: i64,
    ) -> Result<AccessGrant, OAuthServiceError> {
        let record = self.required_token(&subject).await?;
        if token_is_fresh(&record, now) {
            return Ok(access_grant(subject, record));
        }
        self.refresh_record(subject, record, now).await
    }

    pub async fn retry_after_unauthorized(
        &self,
        subject: OAuthSubject,
        rejected_generation: u64,
        now: i64,
    ) -> Result<AccessGrant, OAuthServiceError> {
        let record = self.required_token(&subject).await?;
        if record.generation != rejected_generation {
            return Ok(access_grant(subject, record));
        }
        self.refresh_record(subject, record, now).await
    }

    async fn required_token(
        &self,
        subject: &OAuthSubject,
    ) -> Result<OAuthTokenRecord, OAuthServiceError> {
        self.store()
            .oauth_token(subject.as_str())
            .await?
            .ok_or_else(|| {
                OAuthServiceError::public(
                    OAuthServiceErrorCode::AuthRequired,
                    "还没有绑定落雪 OAuth。请先发送 lxns bind 获取授权链接并完成绑定。",
                )
            })
    }

    async fn refresh_record(
        &self,
        subject: OAuthSubject,
        record: OAuthTokenRecord,
        now: i64,
    ) -> Result<AccessGrant, OAuthServiceError> {
        let client = self.client()?;
        let refresh_token = record.refresh_token().clone();
        let tokens = match client.refresh_token(&refresh_token).await {
            Ok(tokens) => tokens,
            Err(error) if error.code() == OAuthErrorCode::InvalidGrant => {
                let latest = self.required_token(&subject).await?;
                if latest.generation != record.generation {
                    return Ok(access_grant(subject, latest));
                }
                return Err(error.into());
            }
            Err(error) => return Err(error.into()),
        };
        let token = new_stored_token(&tokens, client.config().client_id(), now)?;
        match self
            .store()
            .compare_and_swap_oauth_token(subject.as_str(), record.generation, &token, now)
            .await?
        {
            OAuthCasResult::Stored(record) | OAuthCasResult::Conflict(record) => {
                Ok(access_grant(subject, record))
            }
            OAuthCasResult::Missing => Err(OAuthServiceError::public(
                OAuthServiceErrorCode::AuthRequired,
                "落雪 OAuth 已解绑，请重新授权。",
            )),
        }
    }
}

fn token_is_fresh(record: &OAuthTokenRecord, now: i64) -> bool {
    record
        .expires_at
        .is_none_or(|expires_at| expires_at.saturating_sub(now) > REFRESH_SKEW_SECONDS)
}
