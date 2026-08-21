use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BindTokenArgs {
    pub developer_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchSourceArgs {
    pub qq: Option<String>,
    pub source: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStatusDto {
    pub bound: bool,
    pub updated_at: Option<String>,
    pub security_notice: &'static str,
}

#[derive(Serialize)]
pub struct ClearTokenDto {
    pub bound: bool,
    pub cleared: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchSourceDto {
    pub qq: String,
    pub preferred_source: &'static str,
    pub source_label: &'static str,
    pub text: String,
}
