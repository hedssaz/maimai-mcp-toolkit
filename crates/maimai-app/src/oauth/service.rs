use std::{sync::Arc, time::Duration};

use maimai_providers::{LxnsOAuthClient, OAuthState, PkceVerifier};
use maimai_storage::{
    AuthorizationClaimResult, NewOAuthAuthorization, OAuthConfirmResult, OAuthContext,
    OAuthPendingPoke, StateStore,
};
use secrecy::SecretString;

use super::exchange::{self, PreparePokeSchedule};
use super::{
    AuthorizationLaunch, BindResult, ConfirmResult, ConfirmStatus, OAuthServiceError,
    OAuthServiceErrorCode, OAuthStatus, OAuthSubject, PokeContext, PreparePokeTiming,
    PrepareResult, TrustedOAuthState, UnbindResult, parse::parse_authorization_code,
    tokens::new_stored_token,
};

#[derive(Clone)]
pub struct OAuthService {
    inner: Arc<OAuthServiceInner>,
}

struct OAuthServiceInner {
    store: StateStore,
    client: Option<LxnsOAuthClient>,
}

impl OAuthService {
    pub fn new(store: StateStore, client: LxnsOAuthClient) -> Self {
        Self::from_optional_client(store, Some(client))
    }

    pub fn without_client(store: StateStore) -> Self {
        Self::from_optional_client(store, None)
    }

    fn from_optional_client(store: StateStore, client: Option<LxnsOAuthClient>) -> Self {
        Self {
            inner: Arc::new(OAuthServiceInner { store, client }),
        }
    }

    pub(super) fn client(&self) -> Result<&LxnsOAuthClient, OAuthServiceError> {
        self.inner.client.as_ref().ok_or_else(|| {
            OAuthServiceError::public(
                OAuthServiceErrorCode::ConfigMissing,
                "未配置落雪 OAuth client_id。",
            )
        })
    }

    pub(super) fn store(&self) -> &StateStore {
        &self.inner.store
    }

    pub async fn authorization_url(
        &self,
        subject: OAuthSubject,
        trusted_state: Option<TrustedOAuthState>,
        scopes: Option<&str>,
        context: Option<PokeContext>,
        ttl_seconds: u64,
        now: i64,
    ) -> Result<AuthorizationLaunch, OAuthServiceError> {
        if !(1..=3_600).contains(&ttl_seconds) {
            return Err(invalid("ttlSeconds 必须是 1 到 3600 之间的整数。"));
        }
        let scopes = scopes.map(parse_scopes).transpose()?;
        let explicit = trusted_state.is_some();
        let state = trusted_state
            .map(TrustedOAuthState::into_secret)
            .map(OAuthState::from_secret)
            .transpose()?;
        let request = self
            .client()?
            .authorization_request_with(state, scopes.as_deref())?;
        let (url, state, verifier) = request.into_parts();
        let expires_at = now
            .checked_add(i64::try_from(ttl_seconds).map_err(|_| invalid("ttlSeconds 超出范围。"))?)
            .ok_or_else(|| invalid("ttlSeconds 超出范围。"))?;
        self.store()
            .save_oauth_authorization(&NewOAuthAuthorization::new(
                subject.as_str().to_owned(),
                state.secret().clone(),
                verifier.secret().clone(),
                context.as_ref().map(storage_context),
                now,
                expires_at,
            ))
            .await?;
        Ok(AuthorizationLaunch {
            subject,
            url,
            expires_at,
            requires_poke: explicit,
        })
    }

    pub async fn bind_code(
        &self,
        subject: OAuthSubject,
        code_input: &str,
        explicit_state: Option<TrustedOAuthState>,
        context: Option<PokeContext>,
        now: i64,
    ) -> Result<BindResult, OAuthServiceError> {
        self.bind_code_inner(subject, code_input, explicit_state, context, now, None)
            .await
    }

    pub async fn bind_code_with_timeout(
        &self,
        subject: OAuthSubject,
        code_input: &str,
        explicit_state: Option<TrustedOAuthState>,
        context: Option<PokeContext>,
        now: i64,
        timeout: Duration,
    ) -> Result<BindResult, OAuthServiceError> {
        exchange::validate_timeout(timeout)?;
        self.bind_code_inner(
            subject,
            code_input,
            explicit_state,
            context,
            now,
            Some(timeout),
        )
        .await
    }

    async fn bind_code_inner(
        &self,
        subject: OAuthSubject,
        code_input: &str,
        explicit_state: Option<TrustedOAuthState>,
        context: Option<PokeContext>,
        now: i64,
        timeout: Option<Duration>,
    ) -> Result<BindResult, OAuthServiceError> {
        let client = self.client()?;
        let (claim, code) = self
            .claim_for_code(&subject, code_input, explicit_state, context.as_ref(), now)
            .await?;
        let generation = claim.authorization.generation;
        let verifier = PkceVerifier::from_secret(claim.authorization.code_verifier().clone())?;
        let tokens =
            exchange::tokens(self, &subject, generation, &code, &verifier, timeout).await?;
        let token = new_stored_token(&tokens, client.config().client_id(), now)?;
        let committed = self
            .store()
            .commit_oauth_authorization(subject.as_str(), generation, &token, now)
            .await;
        let record = match committed {
            Ok(Some(record)) => record,
            Ok(None) => {
                return Err(OAuthServiceError::public(
                    OAuthServiceErrorCode::AuthorizationLost,
                    "落雪 OAuth 授权状态已被其他请求处理。",
                ));
            }
            Err(error) => {
                self.store()
                    .release_oauth_authorization_claim(subject.as_str(), generation)
                    .await?;
                return Err(error.into());
            }
        };
        Ok(BindResult {
            subject,
            expires_at: record.expires_at,
            revision: record.generation,
        })
    }

    pub async fn prepare_poke(
        &self,
        subject: OAuthSubject,
        code_input: &str,
        explicit_state: Option<TrustedOAuthState>,
        context: PokeContext,
        ttl_seconds: u64,
        now: i64,
    ) -> Result<PrepareResult, OAuthServiceError> {
        self.prepare_poke_inner(
            subject,
            code_input,
            explicit_state,
            context,
            PreparePokeSchedule {
                ttl_seconds,
                now,
                timeout: None,
            },
        )
        .await
    }

    pub async fn prepare_poke_with_timeout(
        &self,
        subject: OAuthSubject,
        code_input: &str,
        explicit_state: Option<TrustedOAuthState>,
        context: PokeContext,
        timing: PreparePokeTiming,
    ) -> Result<PrepareResult, OAuthServiceError> {
        let (ttl_seconds, now, timeout) = timing.into_parts();
        exchange::validate_timeout(timeout)?;
        self.prepare_poke_inner(
            subject,
            code_input,
            explicit_state,
            context,
            PreparePokeSchedule {
                ttl_seconds,
                now,
                timeout: Some(timeout),
            },
        )
        .await
    }

    async fn prepare_poke_inner(
        &self,
        subject: OAuthSubject,
        code_input: &str,
        explicit_state: Option<TrustedOAuthState>,
        context: PokeContext,
        schedule: PreparePokeSchedule,
    ) -> Result<PrepareResult, OAuthServiceError> {
        let PreparePokeSchedule {
            ttl_seconds,
            now,
            timeout,
        } = schedule;
        if !(1..=1_800).contains(&ttl_seconds) {
            return Err(invalid("ttlSeconds 必须是 1 到 1800 之间的整数。"));
        }
        let client = self.client()?;
        let (claim, code) = self
            .claim_for_code(&subject, code_input, explicit_state, Some(&context), now)
            .await?;
        let generation = claim.authorization.generation;
        let verifier = PkceVerifier::from_secret(claim.authorization.code_verifier().clone())?;
        let tokens =
            exchange::tokens(self, &subject, generation, &code, &verifier, timeout).await?;
        let expires_at = now
            .checked_add(i64::try_from(ttl_seconds).map_err(|_| invalid("ttlSeconds 超出范围。"))?)
            .ok_or_else(|| invalid("ttlSeconds 超出范围。"))?;
        let stored = self
            .store()
            .save_oauth_pending_poke(&OAuthPendingPoke {
                subject: subject.as_str().to_owned(),
                authorization_generation: generation,
                context: storage_context(&context),
                token: new_stored_token(&tokens, client.config().client_id(), now)?,
                created_at: now,
                expires_at,
            })
            .await;
        if let Err(error) = stored {
            self.store()
                .release_oauth_authorization_claim(subject.as_str(), generation)
                .await?;
            return Err(error.into());
        }
        Ok(PrepareResult {
            subject,
            expires_at,
        })
    }

    pub async fn confirm_poke(
        &self,
        subject: OAuthSubject,
        context: PokeContext,
        now: i64,
    ) -> Result<ConfirmResult, OAuthServiceError> {
        let result = self
            .store()
            .confirm_oauth_pending_poke(subject.as_str(), &storage_context(&context), now)
            .await?;
        let (status, expires_at, revision) = match result {
            OAuthConfirmResult::Confirmed(record) => (
                ConfirmStatus::Confirmed,
                record.expires_at,
                Some(record.generation),
            ),
            OAuthConfirmResult::NotFound => (ConfirmStatus::NotFound, None, None),
            OAuthConfirmResult::Expired => (ConfirmStatus::Expired, None, None),
            OAuthConfirmResult::ContextMismatch => (ConfirmStatus::ContextMismatch, None, None),
        };
        Ok(ConfirmResult {
            subject,
            status,
            expires_at,
            revision,
        })
    }

    pub async fn status(
        &self,
        subject: OAuthSubject,
        now: i64,
    ) -> Result<OAuthStatus, OAuthServiceError> {
        let token = self.store().oauth_token(subject.as_str()).await?;
        let pending = self
            .store()
            .oauth_pending_poke(subject.as_str(), now)
            .await?;
        Ok(OAuthStatus {
            subject,
            bound: token.is_some(),
            pending: pending.is_some(),
            expires_at: token.as_ref().and_then(|token| token.expires_at),
            confirmation_expires_at: pending.map(|pending| pending.expires_at),
            revision: token.map_or(0, |token| token.generation),
        })
    }

    pub async fn unbind(&self, subject: OAuthSubject) -> Result<UnbindResult, OAuthServiceError> {
        let changed = self.store().unbind_oauth(subject.as_str()).await?;
        Ok(UnbindResult { subject, changed })
    }

    async fn claim_for_code(
        &self,
        subject: &OAuthSubject,
        input: &str,
        explicit_state: Option<TrustedOAuthState>,
        context: Option<&PokeContext>,
        now: i64,
    ) -> Result<(maimai_storage::AuthorizationClaim, SecretString), OAuthServiceError> {
        let parsed = parse_authorization_code(input)?;
        let explicit_state = explicit_state.map(TrustedOAuthState::into_secret);
        let context = context.map(storage_context);
        match self
            .store()
            .claim_oauth_authorization(
                subject.as_str(),
                explicit_state.as_ref(),
                parsed.state.as_ref(),
                context.as_ref(),
                now,
            )
            .await?
        {
            AuthorizationClaimResult::Claimed(claim) => Ok((claim, parsed.code)),
            AuthorizationClaimResult::NotFound => Err(OAuthServiceError::public(
                OAuthServiceErrorCode::AuthorizationNotFound,
                "没有待处理的落雪 OAuth 授权，请先生成授权链接。",
            )),
            AuthorizationClaimResult::Expired => Err(OAuthServiceError::public(
                OAuthServiceErrorCode::AuthorizationExpired,
                "落雪 OAuth 授权已过期，请重新生成授权链接。",
            )),
            AuthorizationClaimResult::StateMismatch => Err(OAuthServiceError::public(
                OAuthServiceErrorCode::StateMismatch,
                "落雪 OAuth state 不匹配。",
            )),
            AuthorizationClaimResult::ContextMismatch => Err(OAuthServiceError::public(
                OAuthServiceErrorCode::ContextMismatch,
                "OAuth state 上下文不匹配。",
            )),
            AuthorizationClaimResult::Busy => Err(OAuthServiceError::public(
                OAuthServiceErrorCode::ExchangeBusy,
                "落雪 OAuth 授权正在处理中。",
            )),
        }
    }
}

fn parse_scopes(value: &str) -> Result<Vec<String>, OAuthServiceError> {
    let scopes = value
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if scopes.is_empty() {
        return Err(invalid("OAuth scope 格式不正确。"));
    }
    Ok(scopes)
}

fn storage_context(context: &PokeContext) -> OAuthContext {
    OAuthContext {
        adapter_id: context.adapter_id.clone(),
        group_id: context.group_id.clone(),
        bot_qq: context.bot_qq.clone(),
    }
}

fn invalid(message: &'static str) -> OAuthServiceError {
    OAuthServiceError::public(OAuthServiceErrorCode::InvalidInput, message)
}
