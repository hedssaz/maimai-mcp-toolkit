use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OAuthUrlArgs {
    pub qq: Option<String>,
    pub subject: Option<String>,
    pub adapter_id: Option<String>,
    pub adapter: Option<String>,
    pub group_id: Option<String>,
    pub conversation: Option<String>,
    pub bot_qq: Option<String>,
    pub bot: Option<String>,
    pub state: Option<String>,
    pub scopes: Option<String>,
    pub ttl_seconds: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BindCodeArgs {
    pub qq: Option<String>,
    pub subject: Option<String>,
    pub adapter_id: Option<String>,
    pub adapter: Option<String>,
    pub group_id: Option<String>,
    pub conversation: Option<String>,
    pub bot_qq: Option<String>,
    pub bot: Option<String>,
    pub state: Option<String>,
    pub code: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PreparePokeArgs {
    pub qq: Option<String>,
    pub subject: Option<String>,
    pub adapter_id: Option<String>,
    pub adapter: Option<String>,
    pub group_id: Option<String>,
    pub conversation: Option<String>,
    pub bot_qq: Option<String>,
    pub bot: Option<String>,
    pub state: Option<String>,
    pub code: Option<String>,
    pub ttl_seconds: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConfirmPokeArgs {
    pub qq: Option<String>,
    pub subject: Option<String>,
    pub adapter_id: Option<String>,
    pub adapter: Option<String>,
    pub group_id: Option<String>,
    pub conversation: Option<String>,
    pub bot_qq: Option<String>,
    pub bot: Option<String>,
    pub state: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectArgs {
    pub qq: Option<String>,
    pub subject: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationUrlDto {
    pub ok: bool,
    pub authorization_url: String,
    pub expires_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundDto {
    pub ok: bool,
    pub bound: bool,
    pub has_refresh_token: bool,
    pub expires_at: Option<String>,
    pub revision: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDto {
    pub ok: bool,
    pub pending: bool,
    pub confirmation_expires_at: String,
}

pub struct ConfirmDto {
    pub ok: bool,
    pub confirmed: bool,
    pub status: String,
    pub bound: Option<bool>,
    pub has_refresh_token: Option<bool>,
    pub expires_at: Option<Option<String>>,
    pub revision: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDto {
    pub ok: bool,
    pub bound: bool,
    pub pending: bool,
    pub expires_at: Option<String>,
    pub confirmation_expires_at: Option<String>,
    pub revision: u64,
}

#[derive(Serialize)]
pub struct UnbindDto {
    pub ok: bool,
    pub changed: bool,
    pub bound: bool,
    pub pending: bool,
}
