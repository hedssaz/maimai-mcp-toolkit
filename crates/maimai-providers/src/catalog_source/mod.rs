mod config;
mod decode;
mod diving_fish;
mod dxdata;
mod dxrating;
mod error;
mod location;
mod lxns;
#[cfg(test)]
mod mock;
mod request;
mod types;
mod yuzu;

use reqwest::redirect::Policy;

pub use config::CatalogSourceConfig;
pub use error::{CatalogSourceError, CatalogSourceErrorCode, EndpointId};
pub use types::{
    BundleStatus, CatalogSource, DocumentDigest, DocumentStatistics, EntityTag, SourceBundle,
    SourceDocument, SourceMetadata, SourceTarget,
};

use config::EndpointSet;

#[derive(Clone, Debug)]
pub struct CatalogSourceClient {
    http: reqwest::Client,
    config: CatalogSourceConfig,
    endpoints: EndpointSet,
}

impl CatalogSourceClient {
    pub fn new(config: CatalogSourceConfig) -> Result<Self, CatalogSourceError> {
        config.validate()?;
        let http = reqwest::Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(|_| {
                CatalogSourceError::configuration(
                    CatalogSourceErrorCode::InvalidConfiguration,
                    "HTTP client could not be constructed",
                )
            })?;
        Ok(Self {
            http,
            config,
            endpoints: EndpointSet::production()?,
        })
    }

    pub async fn fetch(&self, source: CatalogSource) -> Result<SourceBundle, CatalogSourceError> {
        self.fetch_with_etag(source, None).await
    }

    pub async fn fetch_with_etag(
        &self,
        source: CatalogSource,
        etag: Option<&EntityTag>,
    ) -> Result<SourceBundle, CatalogSourceError> {
        match source {
            CatalogSource::Lxns => lxns::fetch(self).await,
            CatalogSource::DxData => dxdata::fetch(self).await,
            CatalogSource::DivingFish => diving_fish::fetch_music(self, etag).await,
            CatalogSource::Yuzu => yuzu::fetch_aliases(self).await,
            CatalogSource::ChartStats => diving_fish::fetch_stats(self).await,
            CatalogSource::DxRatingAliases => dxrating::fetch_aliases(self).await,
            CatalogSource::DxRatingTags => dxrating::fetch_tags(self).await,
            CatalogSource::Plate => yuzu::fetch_plate(self).await,
            CatalogSource::Location => location::fetch(self).await,
        }
    }

    #[cfg(test)]
    fn with_endpoints(
        config: CatalogSourceConfig,
        endpoints: EndpointSet,
    ) -> Result<Self, CatalogSourceError> {
        config.validate()?;
        let http = reqwest::Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(|_| {
                CatalogSourceError::configuration(
                    CatalogSourceErrorCode::InvalidConfiguration,
                    "HTTP client could not be constructed",
                )
            })?;
        Ok(Self {
            http,
            config,
            endpoints,
        })
    }
}

#[cfg(test)]
mod tests;
