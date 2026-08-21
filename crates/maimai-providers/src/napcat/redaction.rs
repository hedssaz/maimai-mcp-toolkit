use serde_json::Value;
use url::Url;

const MAX_ERROR_BODY_CHARS: usize = 4_096;
const REDACTED: &str = "[REDACTED]";

pub(super) fn sanitize_error_body(body: &str, secrets: &[&str]) -> String {
    let mut sanitized = sanitize_text(body, secrets);
    if let Ok(mut value) = serde_json::from_str::<Value>(&sanitized) {
        redact_json_secrets(&mut value);
        if let Ok(serialized) = serde_json::to_string(&value) {
            sanitized = sanitize_text(&serialized, secrets);
        }
    }
    sanitized.chars().take(MAX_ERROR_BODY_CHARS).collect()
}

pub(super) fn sanitize_text(text: &str, secrets: &[&str]) -> String {
    let mut sanitized = text.to_owned();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        sanitized = sanitized.replace(secret, REDACTED);
    }
    sanitized
}

pub(super) fn safe_endpoint(url: &Url) -> String {
    format!("{}{}", url.origin().ascii_serialization(), url.path())
}

fn redact_json_secrets(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_sensitive_name(key) {
                    *value = Value::String(REDACTED.to_owned());
                } else {
                    redact_json_secrets(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json_secrets(value);
            }
        }
        _ => {}
    }
}

fn is_sensitive_name(name: &str) -> bool {
    let normalized = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    ["token", "secret", "password", "authorization", "cookie"]
        .iter()
        .any(|marker| normalized.contains(marker))
}
