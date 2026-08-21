use std::{io, time::Duration};

use maimai_providers::{CatalogSource, SourceTarget};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshErrorCode {
    InvalidSource,
    InvalidTtl,
    InvalidTimeout,
    InvalidTarget,
    InvalidEntityTag,
    Io,
    Join,
}

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("INVALID_SOURCE: source {catalog_source} is not enabled on this server")]
    InvalidSource { catalog_source: &'static str },

    #[error("ttl_days must be a finite number greater than or equal to zero")]
    InvalidTtl,

    #[error("timeout_seconds must be at least 1")]
    InvalidTimeout,

    #[error("invalid catalog refresh target for {catalog_source}: {target}")]
    InvalidTarget {
        catalog_source: &'static str,
        target: &'static str,
    },

    #[error("stored DivingFish entity tag is invalid")]
    InvalidEntityTag,

    #[error("catalog refresh {operation} failed for {catalog_source}: {target}")]
    Io {
        operation: &'static str,
        catalog_source: &'static str,
        target: &'static str,
        #[source]
        source_error: io::Error,
    },

    #[error("catalog refresh worker failed")]
    Join(#[source] tokio::task::JoinError),
}

impl RefreshError {
    pub const fn code(&self) -> RefreshErrorCode {
        match self {
            Self::InvalidSource { .. } => RefreshErrorCode::InvalidSource,
            Self::InvalidTtl => RefreshErrorCode::InvalidTtl,
            Self::InvalidTimeout => RefreshErrorCode::InvalidTimeout,
            Self::InvalidTarget { .. } => RefreshErrorCode::InvalidTarget,
            Self::InvalidEntityTag => RefreshErrorCode::InvalidEntityTag,
            Self::Io { .. } => RefreshErrorCode::Io,
            Self::Join(_) => RefreshErrorCode::Join,
        }
    }

    pub(crate) fn target(
        source: CatalogSource,
        target: SourceTarget,
        operation: &'static str,
        source_error: io::Error,
    ) -> Self {
        Self::Io {
            operation,
            catalog_source: source.name(),
            target: target.file_name(),
            source_error,
        }
    }

    pub(crate) fn logical(
        source: CatalogSource,
        operation: &'static str,
        source_error: io::Error,
    ) -> Self {
        Self::Io {
            operation,
            catalog_source: source.name(),
            target: "source metadata",
            source_error,
        }
    }
}

pub fn timeout_duration(seconds: u64) -> Result<Duration, RefreshError> {
    if seconds == 0 {
        return Err(RefreshError::InvalidTimeout);
    }
    Ok(Duration::from_secs(seconds))
}
