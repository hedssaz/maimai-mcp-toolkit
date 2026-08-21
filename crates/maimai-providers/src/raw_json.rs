use std::{error::Error, fmt};

use serde_json::Value;

pub const MAX_RAW_JSON_BYTES: usize = 4 * 1024 * 1024;

pub struct RawJsonPayload {
    bytes: Box<[u8]>,
}

impl RawJsonPayload {
    pub fn from_value(value: &Value) -> Result<Self, RawJsonError> {
        let bytes = serde_json::to_vec(value).map_err(|_| RawJsonError::Encode)?;
        if bytes.len() > MAX_RAW_JSON_BYTES {
            return Err(RawJsonError::TooLarge {
                limit: MAX_RAW_JSON_BYTES,
            });
        }
        Ok(Self {
            bytes: bytes.into_boxed_slice(),
        })
    }

    pub fn into_value(self) -> Result<Value, RawJsonError> {
        serde_json::from_slice(&self.bytes).map_err(|_| RawJsonError::Decode)
    }

    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }
}

impl fmt::Debug for RawJsonPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawJsonPayload")
            .field("byte_len", &self.bytes.len())
            .field("content", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawJsonError {
    Encode,
    Decode,
    TooLarge { limit: usize },
}

impl fmt::Display for RawJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode => formatter.write_str("provider raw JSON 编码失败"),
            Self::Decode => formatter.write_str("provider raw JSON 解码失败"),
            Self::TooLarge { limit } => write!(formatter, "provider raw JSON 超过 {limit} 字节"),
        }
    }
}

impl Error for RawJsonError {}

#[derive(Debug)]
pub struct ProviderEnvelope<T> {
    data: T,
    raw: RawJsonPayload,
}

impl<T> ProviderEnvelope<T> {
    pub fn new(data: T, raw: RawJsonPayload) -> Self {
        Self { data, raw }
    }

    pub fn data(&self) -> &T {
        &self.data
    }

    pub fn into_data(self) -> T {
        self.data
    }

    pub fn into_parts(self) -> (T, RawJsonPayload) {
        (self.data, self.raw)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MAX_RAW_JSON_BYTES, RawJsonError, RawJsonPayload};

    #[test]
    fn raw_payload_is_explicit_bounded_and_debug_redacted() -> Result<(), RawJsonError> {
        let secret = "RAW_SECRET_SENTINEL";
        let payload = RawJsonPayload::from_value(&json!({"token": secret}))?;
        assert!(!format!("{payload:?}").contains(secret));
        assert_eq!(payload.into_value()?, json!({"token": secret}));
        let oversized = json!({"value": "x".repeat(MAX_RAW_JSON_BYTES)});
        assert!(matches!(
            RawJsonPayload::from_value(&oversized),
            Err(RawJsonError::TooLarge { .. })
        ));
        Ok(())
    }
}
