use maimai_app::oauth::{
    BindResult, ConfirmResult, ConfirmStatus, OAuthStatus, OAuthSubject, PokeContext,
    PrepareResult, TrustedOAuthState, UnbindResult,
};
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::{dto, error::OAuthToolError};

pub fn subject(
    primary: Option<String>,
    alias: Option<String>,
) -> Result<OAuthSubject, OAuthToolError> {
    OAuthSubject::new(preferred(primary, alias)).map_err(Into::into)
}

pub fn optional_context(
    adapter_id: Option<String>,
    adapter: Option<String>,
    group_id: Option<String>,
    conversation: Option<String>,
    bot_qq: Option<String>,
    bot: Option<String>,
) -> Result<Option<PokeContext>, OAuthToolError> {
    PokeContext::optional(
        preferred(adapter_id, adapter),
        preferred(group_id, conversation),
        preferred(bot_qq, bot),
    )
    .map_err(Into::into)
}

pub fn required_context(
    adapter_id: Option<String>,
    adapter: Option<String>,
    group_id: Option<String>,
    conversation: Option<String>,
    bot_qq: Option<String>,
    bot: Option<String>,
) -> Result<PokeContext, OAuthToolError> {
    PokeContext::new(
        preferred(adapter_id, adapter),
        preferred(group_id, conversation),
        preferred(bot_qq, bot),
    )
    .map_err(Into::into)
}

pub fn trusted_state(
    value: Option<String>,
    subject: &OAuthSubject,
) -> Result<Option<TrustedOAuthState>, OAuthToolError> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| TrustedOAuthState::new(value, subject))
        .transpose()
        .map_err(Into::into)
}

pub fn required_code(value: Option<String>) -> Result<String, OAuthToolError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| OAuthToolError::invalid("OAuth code 格式不正确。"))
}

pub fn authorization_dto(url: String, expires_at: i64) -> Result<Value, OAuthToolError> {
    to_value(dto::AuthorizationUrlDto {
        ok: true,
        authorization_url: url,
        expires_at: timestamp(expires_at)?,
    })
}

pub fn bound_dto(result: BindResult) -> Result<Value, OAuthToolError> {
    to_value(dto::BoundDto {
        ok: true,
        bound: true,
        has_refresh_token: true,
        expires_at: optional_timestamp(result.expires_at)?,
        revision: result.revision,
    })
}

pub fn pending_dto(result: PrepareResult) -> Result<Value, OAuthToolError> {
    to_value(dto::PendingDto {
        ok: true,
        pending: true,
        confirmation_expires_at: timestamp(result.expires_at)?,
    })
}

pub fn confirm_dto(result: ConfirmResult) -> Result<dto::ConfirmDto, OAuthToolError> {
    let confirmed = result.status == ConfirmStatus::Confirmed;
    Ok(dto::ConfirmDto {
        ok: true,
        confirmed,
        status: result.status.as_str().to_owned(),
        bound: confirmed.then_some(true),
        has_refresh_token: confirmed.then_some(true),
        expires_at: if confirmed {
            Some(optional_timestamp(result.expires_at)?)
        } else {
            None
        },
        revision: result.revision,
    })
}

pub fn confirm_value(result: &dto::ConfirmDto) -> Value {
    if result.confirmed {
        json!({
            "ok": result.ok,
            "bound": result.bound,
            "hasRefreshToken": result.has_refresh_token,
            "expiresAt": result.expires_at.as_ref().and_then(Clone::clone),
            "revision": result.revision,
            "confirmed": true,
            "status": result.status,
        })
    } else {
        json!({
            "ok": result.ok,
            "confirmed": false,
            "status": result.status,
        })
    }
}

pub fn status_dto(result: OAuthStatus) -> Result<dto::StatusDto, OAuthToolError> {
    Ok(dto::StatusDto {
        ok: true,
        bound: result.bound,
        pending: result.pending,
        expires_at: optional_timestamp(result.expires_at)?,
        confirmation_expires_at: optional_timestamp(result.confirmation_expires_at)?,
        revision: result.revision,
    })
}

pub fn unbind_dto(result: UnbindResult) -> dto::UnbindDto {
    dto::UnbindDto {
        ok: true,
        changed: result.changed,
        bound: false,
        pending: false,
    }
}

pub fn to_value(value: impl serde::Serialize) -> Result<Value, OAuthToolError> {
    serde_json::to_value(value).map_err(|_| OAuthToolError::internal())
}

fn preferred(primary: Option<String>, alias: Option<String>) -> String {
    primary
        .filter(|value| !value.trim().is_empty())
        .or(alias)
        .unwrap_or_default()
}

fn optional_timestamp(value: Option<i64>) -> Result<Option<String>, OAuthToolError> {
    value.map(timestamp).transpose()
}

fn timestamp(value: i64) -> Result<String, OAuthToolError> {
    let timestamp =
        OffsetDateTime::from_unix_timestamp(value).map_err(|_| OAuthToolError::internal())?;
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
        timestamp.year(),
        u8::from(timestamp.month()),
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute(),
        timestamp.second(),
    ))
}
