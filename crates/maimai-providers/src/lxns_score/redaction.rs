use serde_json::Value;

const MAX_ERROR_BODY_CHARS: usize = 4_096;
const REDACTED: &str = "[REDACTED]";

pub(super) fn sanitize_error_body(body: &str, access_token: &str) -> String {
    let mut sanitized = redact_access_token(body.to_owned(), access_token);
    if let Ok(mut value) = serde_json::from_str::<Value>(&sanitized) {
        redact_json_secrets(&mut value, access_token);
        if let Ok(serialized) = serde_json::to_string(&value) {
            sanitized = redact_access_token(serialized, access_token);
        }
    }
    sanitized.chars().take(MAX_ERROR_BODY_CHARS).collect()
}

fn redact_access_token(mut text: String, access_token: &str) -> String {
    if access_token.is_empty() {
        return text;
    }
    text = text.replace(access_token, REDACTED);
    if let Ok(encoded) = serde_json::to_string(access_token)
        && let Some(encoded) = encoded
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
    {
        text = text.replace(encoded, REDACTED);
    }
    text
}

fn redact_json_secrets(value: &mut Value, access_token: &str) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_sensitive_name(key) {
                    *value = Value::String(REDACTED.to_owned());
                } else {
                    redact_json_secrets(value, access_token);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json_secrets(value, access_token);
            }
        }
        Value::String(text) if !access_token.is_empty() => {
            *text = text.replace(access_token, REDACTED);
        }
        Value::String(_) => {}
        _ => {}
    }
}

fn is_sensitive_name(name: &str) -> bool {
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
