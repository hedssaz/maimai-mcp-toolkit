use secrecy::SecretString;
use url::{Url, form_urlencoded};

use super::{OAuthServiceError, OAuthServiceErrorCode};

pub(super) struct ParsedAuthorizationCode {
    pub code: SecretString,
    pub state: Option<SecretString>,
}

impl std::fmt::Debug for ParsedAuthorizationCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParsedAuthorizationCode")
            .field("has_code", &true)
            .field("has_state", &self.state.is_some())
            .finish()
    }
}

pub(super) fn parse_authorization_code(
    input: &str,
) -> Result<ParsedAuthorizationCode, OAuthServiceError> {
    let input = input.trim();
    if input.is_empty() || input.len() > 2_048 {
        return Err(invalid("OAuth code 格式不正确。"));
    }
    if let Ok(url) = Url::parse(input) {
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(invalid("OAuth callback URL 格式不正确。"));
        }
        if url.fragment().is_some() {
            return Err(invalid("OAuth callback URL 参数存在歧义。"));
        }
        let Some(query) = url.query() else {
            return Err(invalid("OAuth callback URL 缺少 code。"));
        };
        return from_pairs(form_urlencoded::parse(query.as_bytes()).into_owned(), true);
    }
    if input.contains("://") {
        return Err(invalid("OAuth callback URL 格式不正确。"));
    }
    if input.starts_with("code=") || input.starts_with("state=") || input.starts_with("error=") {
        return from_pairs(form_urlencoded::parse(input.as_bytes()).into_owned(), false);
    }
    if input.chars().any(char::is_whitespace) || input.chars().any(char::is_control) {
        return Err(invalid("OAuth code 格式不正确。"));
    }
    Ok(ParsedAuthorizationCode {
        code: SecretString::from(input.to_owned()),
        state: None,
    })
}

fn from_pairs(
    pairs: impl IntoIterator<Item = (String, String)>,
    callback_url: bool,
) -> Result<ParsedAuthorizationCode, OAuthServiceError> {
    let mut code = None;
    let mut state = None;
    let mut provider_error = None;
    for (index, (name, value)) in pairs.into_iter().enumerate() {
        if index >= 8 {
            return Err(invalid("OAuth callback 参数过多。"));
        }
        match name.as_str() {
            "code" => assign_once(&mut code, value, "OAuth callback 参数存在歧义。")?,
            "state" => assign_once(&mut state, value, "OAuth callback 参数存在歧义。")?,
            "error" => assign_once(&mut provider_error, value, "OAuth callback 参数存在歧义。")?,
            _ => {}
        }
    }
    if provider_error.is_some() {
        return Err(OAuthServiceError::public(
            OAuthServiceErrorCode::OAuthRejected,
            "落雪 OAuth 授权未完成。",
        ));
    }
    let code = code
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid(if callback_url {
                "OAuth callback URL 缺少 code。"
            } else {
                "OAuth code 格式不正确。"
            })
        })?;
    if code.chars().any(char::is_whitespace) || code.chars().any(char::is_control) {
        return Err(invalid("OAuth code 格式不正确。"));
    }
    let state = state
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if state.as_ref().is_some_and(|value| {
        value.len() > 1_024
            || value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
    }) {
        return Err(OAuthServiceError::public(
            OAuthServiceErrorCode::InvalidState,
            "OAuth state 格式不正确。",
        ));
    }
    Ok(ParsedAuthorizationCode {
        code: SecretString::from(code),
        state: state.map(SecretString::from),
    })
}

fn assign_once(
    destination: &mut Option<String>,
    value: String,
    duplicate_message: &'static str,
) -> Result<(), OAuthServiceError> {
    if destination.is_some() {
        return Err(invalid(duplicate_message));
    }
    *destination = Some(value);
    Ok(())
}

fn invalid(message: &'static str) -> OAuthServiceError {
    OAuthServiceError::public(OAuthServiceErrorCode::InvalidInput, message)
}
