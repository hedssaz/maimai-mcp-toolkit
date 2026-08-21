use std::{fmt, time::Duration};

use secrecy::SecretString;
use url::Url;

use super::{OAuthServiceError, OAuthServiceErrorCode};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OAuthSubject(String);

impl OAuthSubject {
    pub fn new(value: impl Into<String>) -> Result<Self, OAuthServiceError> {
        let subject = value.into().trim().to_owned();
        if subject.is_empty() || subject.len() > 256 || subject.chars().any(char::is_control) {
            return Err(OAuthServiceError::public(
                OAuthServiceErrorCode::InvalidInput,
                "subject 格式不正确。",
            ));
        }
        Ok(Self(subject))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PokeContext {
    pub adapter_id: String,
    pub group_id: String,
    pub bot_qq: String,
}

impl PokeContext {
    pub fn new(
        adapter_id: impl Into<String>,
        group_id: impl Into<String>,
        bot_qq: impl Into<String>,
    ) -> Result<Self, OAuthServiceError> {
        Ok(Self {
            adapter_id: context_value(adapter_id.into(), "adapter", true)?,
            group_id: context_value(group_id.into(), "conversation", true)?,
            bot_qq: context_value(bot_qq.into(), "bot", true)?,
        })
    }

    pub fn optional(
        adapter_id: impl Into<String>,
        group_id: impl Into<String>,
        bot_qq: impl Into<String>,
    ) -> Result<Option<Self>, OAuthServiceError> {
        let adapter_id = context_value(adapter_id.into(), "adapter", false)?;
        let group_id = context_value(group_id.into(), "conversation", false)?;
        let bot_qq = context_value(bot_qq.into(), "bot", false)?;
        if adapter_id.is_empty() && group_id.is_empty() && bot_qq.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            adapter_id,
            group_id,
            bot_qq,
        }))
    }
}

pub struct TrustedOAuthState(SecretString);

impl TrustedOAuthState {
    pub fn new(
        value: impl Into<String>,
        subject: &OAuthSubject,
    ) -> Result<Self, OAuthServiceError> {
        let value = value.into().trim().to_owned();
        if value.is_empty()
            || value.len() > 1_024
            || value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
            || (subject.as_str().len() >= 5 && value.contains(subject.as_str()))
        {
            return Err(OAuthServiceError::public(
                OAuthServiceErrorCode::InvalidState,
                "OAuth state 必须是不含 subject 的 opaque 值。",
            ));
        }
        Ok(Self(SecretString::from(value)))
    }

    pub(super) fn into_secret(self) -> SecretString {
        self.0
    }
}

impl fmt::Debug for TrustedOAuthState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TrustedOAuthState([REDACTED])")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AuthorizationLaunch {
    pub subject: OAuthSubject,
    pub url: Url,
    pub expires_at: i64,
    pub requires_poke: bool,
}

impl fmt::Debug for AuthorizationLaunch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizationLaunch")
            .field("subject", &self.subject)
            .field("has_authorization_url", &true)
            .field("expires_at", &self.expires_at)
            .field("requires_poke", &self.requires_poke)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindResult {
    pub subject: OAuthSubject,
    pub expires_at: Option<i64>,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareResult {
    pub subject: OAuthSubject,
    pub expires_at: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct PreparePokeTiming {
    ttl_seconds: u64,
    now: i64,
    exchange_timeout: Duration,
}

impl PreparePokeTiming {
    pub const fn new(ttl_seconds: u64, now: i64, exchange_timeout: Duration) -> Self {
        Self {
            ttl_seconds,
            now,
            exchange_timeout,
        }
    }

    pub(super) const fn into_parts(self) -> (u64, i64, Duration) {
        (self.ttl_seconds, self.now, self.exchange_timeout)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmStatus {
    Confirmed,
    NotFound,
    Expired,
    ContextMismatch,
}

impl ConfirmStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::NotFound => "not_found",
            Self::Expired => "expired",
            Self::ContextMismatch => "context_mismatch",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmResult {
    pub subject: OAuthSubject,
    pub status: ConfirmStatus,
    pub expires_at: Option<i64>,
    pub revision: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthStatus {
    pub subject: OAuthSubject,
    pub bound: bool,
    pub pending: bool,
    pub expires_at: Option<i64>,
    pub confirmation_expires_at: Option<i64>,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnbindResult {
    pub subject: OAuthSubject,
    pub changed: bool,
}

pub struct AccessGrant {
    subject: OAuthSubject,
    generation: u64,
    access_token: SecretString,
    expires_at: Option<i64>,
}

impl AccessGrant {
    pub(super) fn new(
        subject: OAuthSubject,
        generation: u64,
        access_token: SecretString,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            subject,
            generation,
            access_token,
            expires_at,
        }
    }

    pub fn subject(&self) -> &OAuthSubject {
        &self.subject
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn access_token(&self) -> &SecretString {
        &self.access_token
    }

    pub const fn expires_at(&self) -> Option<i64> {
        self.expires_at
    }
}

impl fmt::Debug for AccessGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessGrant")
            .field("subject", &self.subject)
            .field("generation", &self.generation)
            .field("has_access_token", &true)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

fn context_value(
    value: String,
    field: &'static str,
    required: bool,
) -> Result<String, OAuthServiceError> {
    let value = value.trim().to_owned();
    if required && value.is_empty() {
        return Err(OAuthServiceError::public(
            OAuthServiceErrorCode::InvalidInput,
            match field {
                "adapter" => "缺少 adapter。",
                "conversation" => "缺少 conversation。",
                _ => "缺少 bot。",
            },
        ));
    }
    if value.chars().count() > 512 || value.chars().any(char::is_control) {
        return Err(OAuthServiceError::public(
            OAuthServiceErrorCode::InvalidInput,
            match field {
                "adapter" => "adapter 格式不正确。",
                "conversation" => "conversation 格式不正确。",
                _ => "bot 格式不正确。",
            },
        ));
    }
    Ok(value)
}
