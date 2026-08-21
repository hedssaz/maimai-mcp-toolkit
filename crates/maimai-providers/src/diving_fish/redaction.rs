use secrecy::ExposeSecret;
use serde_json::Value;

use super::DivingFishRequest;

const MAX_ERROR_BODY_CHARS: usize = 4_096;
const REDACTED: &str = "[REDACTED]";

pub(super) fn secret_redactions(request: &DivingFishRequest) -> Vec<String> {
    let mut values = [
        request.credentials.login_password.as_ref(),
        request.credentials.developer_token.as_ref(),
        request.credentials.import_token.as_ref(),
        request.credentials.jwt_token.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(|secret| secret.expose_secret().to_owned())
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>();

    for (name, value) in &request.headers {
        if is_sensitive_name(name.as_str())
            && let Ok(value) = value.to_str()
            && !value.is_empty()
        {
            values.push(value.to_owned());
        }
    }
    values
}

pub(super) fn sanitize_error_body(body: &str, secrets: &[String]) -> String {
    let mut sanitized = redact_known_secrets(body.to_owned(), secrets);

    if let Ok(mut value) = serde_json::from_str::<Value>(&sanitized) {
        redact_json_secrets(&mut value, secrets);
        if let Ok(serialized) = serde_json::to_string(&value) {
            sanitized = redact_known_secrets(serialized, secrets);
        }
    }

    sanitized.chars().take(MAX_ERROR_BODY_CHARS).collect()
}

fn redact_known_secrets(mut text: String, secrets: &[String]) -> String {
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        text = text.replace(secret, REDACTED);
        if let Ok(encoded) = serde_json::to_string(secret)
            && let Some(encoded) = encoded
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        {
            text = text.replace(encoded, REDACTED);
        }
    }
    text
}

fn redact_json_secrets(value: &mut Value, secrets: &[String]) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_sensitive_name(key) {
                    *value = Value::String(REDACTED.to_owned());
                } else {
                    redact_json_secrets(value, secrets);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json_secrets(value, secrets);
            }
        }
        Value::String(text) => {
            for secret in secrets {
                if !secret.is_empty() {
                    *text = text.replace(secret, REDACTED);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn is_sensitive_name(name: &str) -> bool {
    let normalized = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "token",
        "password",
        "secret",
        "authorization",
        "cookie",
        "apikey",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}
