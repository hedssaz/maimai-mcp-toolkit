use std::fmt;

use secrecy::SecretString;
use serde::Deserialize;

pub struct OAuthTokens {
    pub(super) access_token: SecretString,
    pub(super) refresh_token: SecretString,
    pub(super) token_type: String,
    pub(super) scope: Option<String>,
    pub(super) expires_in: Option<u64>,
}

impl OAuthTokens {
    pub fn access_token(&self) -> &SecretString {
        &self.access_token
    }

    pub fn refresh_token(&self) -> &SecretString {
        &self.refresh_token
    }

    pub fn token_type(&self) -> &str {
        &self.token_type
    }

    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    pub fn expires_in(&self) -> Option<u64> {
        self.expires_in
    }
}

impl fmt::Debug for OAuthTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthTokens")
            .field("has_access_token", &true)
            .field("has_refresh_token", &true)
            .field("has_scope", &self.scope.is_some())
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum SuccessfulResponse {
    Wrapped { data: TokenWire },
    Direct(TokenWire),
}

impl SuccessfulResponse {
    pub(super) fn into_token(self) -> TokenWire {
        match self {
            Self::Wrapped { data } | Self::Direct(data) => data,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct TokenWire {
    #[serde(alias = "accessToken")]
    pub(super) access_token: SecretString,
    #[serde(alias = "refreshToken")]
    pub(super) refresh_token: SecretString,
    #[serde(default, alias = "tokenType")]
    pub(super) token_type: Option<String>,
    #[serde(default)]
    pub(super) scope: Option<String>,
    #[serde(default, alias = "expiresIn")]
    pub(super) expires_in: Option<u64>,
}
