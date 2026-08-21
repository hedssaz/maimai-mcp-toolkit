use std::fmt;

use secrecy::{ExposeSecret, SecretString};

#[derive(Default)]
pub struct DivingFishCredentials {
    pub(super) login_username: Option<String>,
    pub(super) login_password: Option<SecretString>,
    pub(super) developer_token: Option<SecretString>,
    pub(super) import_token: Option<SecretString>,
    pub(super) jwt_token: Option<SecretString>,
}

impl DivingFishCredentials {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_login(
        mut self,
        username: impl Into<String>,
        password: impl Into<SecretString>,
    ) -> Self {
        self.login_username = Some(username.into());
        self.login_password = Some(password.into());
        self
    }

    pub fn with_developer_token(mut self, token: impl Into<SecretString>) -> Self {
        self.developer_token = Some(token.into());
        self
    }

    pub fn with_import_token(mut self, token: impl Into<SecretString>) -> Self {
        self.import_token = Some(token.into());
        self
    }

    pub fn with_jwt_token(mut self, token: impl Into<SecretString>) -> Self {
        self.jwt_token = Some(token.into());
        self
    }

    pub(super) fn has_login(&self) -> bool {
        self.login_username
            .as_deref()
            .is_some_and(|value| !value.is_empty())
            && secret_is_present(self.login_password.as_ref())
    }
}

impl fmt::Debug for DivingFishCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DivingFishCredentials")
            .field("has_login", &self.has_login())
            .field(
                "has_developer_token",
                &secret_is_present(self.developer_token.as_ref()),
            )
            .field(
                "has_import_token",
                &secret_is_present(self.import_token.as_ref()),
            )
            .field("has_jwt_token", &secret_is_present(self.jwt_token.as_ref()))
            .finish()
    }
}

pub(super) fn secret_is_present(secret: Option<&SecretString>) -> bool {
    secret.is_some_and(|value| !value.expose_secret().is_empty())
}
