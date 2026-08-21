use std::time::Duration;

use reqwest::header::{COOKIE, HeaderMap, HeaderName, HeaderValue};
use secrecy::{ExposeSecret, SecretString};

use super::super::{
    AuthRequirement, DivingFishCredentials, DivingFishOperation, DivingFishRequest, ProviderError,
    ProviderErrorCode, credentials::secret_is_present, redaction::is_sensitive_name,
};

static DEVELOPER_TOKEN: HeaderName = HeaderName::from_static("developer-token");
static IMPORT_TOKEN: HeaderName = HeaderName::from_static("import-token");
const MIN_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_TIMEOUT: Duration = Duration::from_secs(300);

pub(super) fn validate_request(request: &DivingFishRequest) -> Result<(), ProviderError> {
    let metadata = request.operation.metadata();
    if request
        .headers
        .keys()
        .any(|name| is_sensitive_name(name.as_str()))
    {
        return Err(ProviderError::new(
            ProviderErrorCode::InvalidRequest,
            "认证 header 必须通过 typed credentials 提供",
        ));
    }
    if metadata.mutating && request.confirm != Some(request.operation) {
        return Err(ProviderError::new(
            ProviderErrorCode::ConfirmationRequired,
            format!(
                "该操作会修改数据；confirm 必须精确匹配 {}",
                request.operation
            ),
        ));
    }
    if !(MIN_TIMEOUT..=MAX_TIMEOUT).contains(&request.timeout) {
        return Err(ProviderError::new(
            ProviderErrorCode::InvalidRequest,
            "timeout 必须在 0.1 到 300 秒之间",
        ));
    }
    if request.body.is_some() && request.raw_body.is_some() {
        return Err(ProviderError::new(
            ProviderErrorCode::InvalidRequest,
            "body 与 rawBody 不能同时提供",
        ));
    }
    if request.raw_body.is_some() && !metadata.raw_body_allowed {
        return Err(ProviderError::new(
            ProviderErrorCode::InvalidRequest,
            "该 operation 不接受 rawBody",
        ));
    }
    if (request.body.is_some() || request.raw_body.is_some()) && !metadata.method.accepts_body() {
        return Err(ProviderError::new(
            ProviderErrorCode::InvalidRequest,
            "GET operation 不接受 body",
        ));
    }
    if request.operation == DivingFishOperation::MaimaiLogin && request.body.is_some() {
        return Err(ProviderError::new(
            ProviderErrorCode::InvalidRequest,
            "maimai_login 必须通过 typed login credentials 提供用户名和密码",
        ));
    }
    validate_auth(metadata.auth, &request.credentials)
}

pub(super) fn apply_headers(
    headers: &mut HeaderMap,
    credentials: &DivingFishCredentials,
) -> Result<(), ProviderError> {
    if let Some(token) = credentials.developer_token.as_ref() {
        headers.insert(DEVELOPER_TOKEN.clone(), secret_header_value(token)?);
    }
    if let Some(token) = credentials.import_token.as_ref() {
        headers.insert(IMPORT_TOKEN.clone(), secret_header_value(token)?);
    }
    if let Some(token) = credentials.jwt_token.as_ref() {
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("jwt_token={}", token.expose_secret())).map_err(
                |_| {
                    ProviderError::new(
                        ProviderErrorCode::InvalidRequest,
                        "jwt token 不能编码为 HTTP header",
                    )
                },
            )?,
        );
    }
    Ok(())
}

pub(super) fn auth_required() -> ProviderError {
    ProviderError::new(ProviderErrorCode::AuthRequired, "该 operation 缺少所需凭据")
}

fn validate_auth(
    requirement: AuthRequirement,
    credentials: &DivingFishCredentials,
) -> Result<(), ProviderError> {
    let valid = match requirement {
        AuthRequirement::None => true,
        AuthRequirement::LoginCredentials => credentials.has_login(),
        AuthRequirement::Login => secret_is_present(credentials.jwt_token.as_ref()),
        AuthRequirement::LoginOrImportToken => {
            secret_is_present(credentials.jwt_token.as_ref())
                || secret_is_present(credentials.import_token.as_ref())
        }
        AuthRequirement::DeveloperToken => secret_is_present(credentials.developer_token.as_ref()),
    };
    valid.then_some(()).ok_or_else(auth_required)
}

fn secret_header_value(secret: &SecretString) -> Result<HeaderValue, ProviderError> {
    HeaderValue::from_str(secret.expose_secret()).map_err(|_| {
        ProviderError::new(
            ProviderErrorCode::InvalidRequest,
            "token 不能编码为 HTTP header",
        )
    })
}
