use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{TryRng as _, rngs::SysRng};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest as _, Sha256};

use super::{OAuthError, OAuthErrorCode};

const STATE_BYTES: usize = 24;
const MAX_STATE_LENGTH: usize = 1_024;
const VERIFIER_BYTES: usize = 32;

pub struct OAuthState(SecretString);

impl OAuthState {
    pub(super) fn generate() -> Result<Self, OAuthError> {
        random_token::<STATE_BYTES>().map(Self)
    }

    pub fn from_secret(secret: SecretString) -> Result<Self, OAuthError> {
        let value = secret.expose_secret();
        if value.is_empty()
            || value.chars().count() > MAX_STATE_LENGTH
            || value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(OAuthError::new(
                OAuthErrorCode::InvalidRequest,
                "OAuth state 格式不正确",
            ));
        }
        Ok(Self(secret))
    }

    pub fn secret(&self) -> &SecretString {
        &self.0
    }
}

impl fmt::Debug for OAuthState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OAuthState([REDACTED])")
    }
}

pub struct PkceVerifier(SecretString);

impl PkceVerifier {
    pub(super) fn generate() -> Result<Self, OAuthError> {
        random_token::<VERIFIER_BYTES>().map(Self)
    }

    pub fn from_secret(secret: SecretString) -> Result<Self, OAuthError> {
        let value = secret.expose_secret();
        if !(43..=128).contains(&value.len())
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
            })
        {
            return Err(OAuthError::new(
                OAuthErrorCode::InvalidRequest,
                "PKCE verifier 格式不正确",
            ));
        }
        Ok(Self(secret))
    }

    pub fn secret(&self) -> &SecretString {
        &self.0
    }

    pub(super) fn challenge(&self) -> String {
        let digest = Sha256::digest(self.0.expose_secret().as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }
}

impl fmt::Debug for PkceVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PkceVerifier([REDACTED])")
    }
}

fn random_token<const N: usize>() -> Result<SecretString, OAuthError> {
    let mut bytes = [0_u8; N];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| OAuthError::new(OAuthErrorCode::EntropyUnavailable, "系统随机数生成失败"))?;
    Ok(SecretString::from(URL_SAFE_NO_PAD.encode(bytes)))
}
