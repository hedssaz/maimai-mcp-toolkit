mod client;
mod credentials;
mod error;
mod operation;
mod redaction;
mod request;

pub(crate) const MAX_RESPONSE_BODY_BYTES: usize = 32 * 1024 * 1024;

pub use client::{DivingFishClient, DivingFishResponse};
pub use credentials::DivingFishCredentials;
pub use error::{ProviderError, ProviderErrorCode};
pub use operation::{
    AuthRequirement, DivingFishGame, DivingFishOperation, HttpMethod, OperationMetadata,
    UnknownOperation,
};
pub use request::{DivingFishRequest, QueryValue};

#[cfg(test)]
mod tests;
