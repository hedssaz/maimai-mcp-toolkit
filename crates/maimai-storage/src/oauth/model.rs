use secrecy::SecretString;

pub(super) fn is_valid_subject(subject: &str) -> bool {
    !subject.is_empty() && subject.len() <= 256 && !subject.chars().any(char::is_control)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthContext {
    pub adapter_id: String,
    pub group_id: String,
    pub bot_qq: String,
}

#[derive(Clone, Debug)]
pub struct NewOAuthAuthorization {
    pub subject: String,
    pub(super) state: SecretString,
    pub(super) code_verifier: SecretString,
    pub context: Option<OAuthContext>,
    pub created_at: i64,
    pub expires_at: i64,
}

impl NewOAuthAuthorization {
    pub fn new(
        subject: String,
        state: SecretString,
        code_verifier: SecretString,
        context: Option<OAuthContext>,
        created_at: i64,
        expires_at: i64,
    ) -> Self {
        Self {
            subject,
            state,
            code_verifier,
            context,
            created_at,
            expires_at,
        }
    }

    pub(super) fn state(&self) -> &SecretString {
        &self.state
    }

    pub(super) fn code_verifier(&self) -> &SecretString {
        &self.code_verifier
    }
}

#[derive(Clone, Debug)]
pub struct OAuthAuthorization {
    pub subject: String,
    pub generation: u64,
    pub(super) code_verifier: SecretString,
    pub context: Option<OAuthContext>,
    pub created_at: i64,
    pub expires_at: i64,
}

impl OAuthAuthorization {
    pub fn code_verifier(&self) -> &SecretString {
        &self.code_verifier
    }
}

#[derive(Clone, Debug)]
pub struct AuthorizationClaim {
    pub authorization: OAuthAuthorization,
}

#[derive(Clone, Debug)]
pub enum AuthorizationClaimResult {
    Claimed(AuthorizationClaim),
    NotFound,
    Expired,
    StateMismatch,
    ContextMismatch,
    Busy,
}

#[derive(Clone, Debug)]
pub struct NewOAuthToken {
    pub(super) access_token: SecretString,
    pub(super) refresh_token: SecretString,
    pub token_type: String,
    pub scope: Option<String>,
    pub client_id: String,
    pub expires_at: Option<i64>,
}

impl NewOAuthToken {
    pub fn new(
        access_token: SecretString,
        refresh_token: SecretString,
        token_type: String,
        scope: Option<String>,
        client_id: String,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            token_type,
            scope,
            client_id,
            expires_at,
        }
    }

    pub(super) fn access_token(&self) -> &SecretString {
        &self.access_token
    }

    pub(super) fn refresh_token(&self) -> &SecretString {
        &self.refresh_token
    }
}

#[derive(Clone, Debug)]
pub struct OAuthTokenRecord {
    pub subject: String,
    pub generation: u64,
    pub(super) access_token: SecretString,
    pub(super) refresh_token: SecretString,
    pub token_type: String,
    pub scope: Option<String>,
    pub client_id: String,
    pub expires_at: Option<i64>,
    pub bound_at: i64,
    pub updated_at: i64,
}

impl OAuthTokenRecord {
    pub fn access_token(&self) -> &SecretString {
        &self.access_token
    }

    pub fn refresh_token(&self) -> &SecretString {
        &self.refresh_token
    }

    pub fn into_access_token(self) -> SecretString {
        self.access_token
    }
}

#[derive(Clone, Debug)]
pub struct OAuthPendingPoke {
    pub subject: String,
    pub authorization_generation: u64,
    pub context: OAuthContext,
    pub token: NewOAuthToken,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Debug)]
pub enum OAuthConfirmResult {
    Confirmed(OAuthTokenRecord),
    NotFound,
    Expired,
    ContextMismatch,
}

#[derive(Clone, Debug)]
pub enum OAuthCasResult {
    Stored(OAuthTokenRecord),
    Conflict(OAuthTokenRecord),
    Missing,
}
