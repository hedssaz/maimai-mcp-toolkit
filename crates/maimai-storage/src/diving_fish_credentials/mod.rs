mod schema;
mod store;

use secrecy::SecretString;
use time::OffsetDateTime;

pub(crate) use schema::initialize_diving_fish_credentials_schema;

#[derive(Clone, Debug)]
pub struct DivingFishDeveloperToken {
    token: SecretString,
    pub updated_at: OffsetDateTime,
}

impl DivingFishDeveloperToken {
    pub fn token(&self) -> &SecretString {
        &self.token
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeveloperTokenMetadata {
    pub updated_at: OffsetDateTime,
}

#[cfg(test)]
mod tests;
