use std::fmt;

use url::Url;

use super::{OAuthState, PkceVerifier, redaction::safe_endpoint};

pub struct AuthorizationRequest {
    pub(super) url: Url,
    pub(super) state: OAuthState,
    pub(super) code_verifier: PkceVerifier,
}

impl AuthorizationRequest {
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn state(&self) -> &OAuthState {
        &self.state
    }

    pub fn code_verifier(&self) -> &PkceVerifier {
        &self.code_verifier
    }

    pub fn into_parts(self) -> (Url, OAuthState, PkceVerifier) {
        (self.url, self.state, self.code_verifier)
    }
}

impl fmt::Debug for AuthorizationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizationRequest")
            .field("endpoint", &safe_endpoint(&self.url))
            .field("state", &self.state)
            .field("code_verifier", &self.code_verifier)
            .finish()
    }
}
