use std::time::Duration;

use reqwest::header::HeaderValue;
use url::Url;

use super::error::{CatalogSourceError, CatalogSourceErrorCode, EndpointId};
use super::types::CatalogSource;

const DEFAULT_BODY_LIMIT: usize = 32 * 1024 * 1024;
const DEFAULT_USER_AGENT: &str = "maimai-catalog-source/0.1";

#[derive(Clone, Debug)]
pub struct CatalogSourceConfig {
    max_body_bytes: usize,
    timeout_override: Option<Duration>,
    user_agent: String,
}

impl Default for CatalogSourceConfig {
    fn default() -> Self {
        Self {
            max_body_bytes: DEFAULT_BODY_LIMIT,
            timeout_override: None,
            user_agent: DEFAULT_USER_AGENT.to_owned(),
        }
    }
}

impl CatalogSourceConfig {
    pub fn new(user_agent: impl Into<String>, max_body_bytes: usize) -> Self {
        Self {
            max_body_bytes,
            timeout_override: None,
            user_agent: user_agent.into(),
        }
    }

    pub fn with_timeout_override(mut self, timeout: Duration) -> Self {
        self.timeout_override = Some(timeout);
        self
    }

    pub const fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }

    pub const fn timeout_override(&self) -> Option<Duration> {
        self.timeout_override
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub(crate) fn validate(&self) -> Result<(), CatalogSourceError> {
        if self.max_body_bytes == 0 {
            return Err(CatalogSourceError::configuration(
                CatalogSourceErrorCode::InvalidConfiguration,
                "body size limit must be greater than zero",
            ));
        }
        if self
            .timeout_override
            .is_some_and(|timeout| timeout.is_zero())
        {
            return Err(CatalogSourceError::configuration(
                CatalogSourceErrorCode::InvalidConfiguration,
                "timeout must be greater than zero",
            ));
        }
        HeaderValue::from_str(&self.user_agent).map_err(|_| {
            CatalogSourceError::configuration(
                CatalogSourceErrorCode::InvalidConfiguration,
                "user agent is not a valid HTTP header value",
            )
        })?;
        if self.user_agent.trim().is_empty() {
            return Err(CatalogSourceError::configuration(
                CatalogSourceErrorCode::InvalidConfiguration,
                "user agent must not be empty",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EndpointSpec {
    pub(crate) id: EndpointId,
    pub(crate) source: CatalogSource,
    pub(crate) url: Url,
    pub(crate) timeout: Duration,
    pub(crate) user_agent: Option<&'static str>,
    pub(crate) accepts_text: bool,
}

impl EndpointSpec {
    fn parse(
        id: EndpointId,
        source: CatalogSource,
        url: &str,
        timeout_seconds: u64,
        user_agent: Option<&'static str>,
        accepts_text: bool,
    ) -> Result<Self, CatalogSourceError> {
        let url = Url::parse(url).map_err(|_| invalid_endpoint())?;
        validate_endpoint_url(&url)?;
        Ok(Self {
            id,
            source,
            url,
            timeout: Duration::from_secs(timeout_seconds),
            user_agent,
            accepts_text,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EndpointSet {
    endpoints: Vec<EndpointSpec>,
}

impl EndpointSet {
    pub(crate) fn production() -> Result<Self, CatalogSourceError> {
        let definitions = [
            (
                EndpointId::LxnsSongs,
                CatalogSource::Lxns,
                "https://maimai.lxns.net/api/v0/maimai/song/list",
                30,
                None,
                false,
            ),
            (
                EndpointId::LxnsAliases,
                CatalogSource::Lxns,
                "https://maimai.lxns.net/api/v0/maimai/alias/list",
                30,
                None,
                false,
            ),
            (
                EndpointId::DxDataProxy,
                CatalogSource::DxData,
                "https://ghproxy.net/https://raw.githubusercontent.com/gekichumai/dxrating/main/packages/dxdata/dxdata.json",
                30,
                Some("maimai-bot dxdata-updater"),
                true,
            ),
            (
                EndpointId::DxDataGithub,
                CatalogSource::DxData,
                "https://raw.githubusercontent.com/gekichumai/dxrating/main/packages/dxdata/dxdata.json",
                30,
                Some("maimai-bot dxdata-updater"),
                true,
            ),
            (
                EndpointId::DivingFishMusic,
                CatalogSource::DivingFish,
                "https://www.diving-fish.com/api/maimaidxprober/music_data",
                30,
                None,
                false,
            ),
            (
                EndpointId::YuzuAliases,
                CatalogSource::Yuzu,
                "https://www.yuzuchan.moe/api/maimaidx/maimaidxalias",
                5,
                Some("maimai-bot yuzu-alias-updater"),
                false,
            ),
            (
                EndpointId::DivingFishChartStats,
                CatalogSource::ChartStats,
                "https://www.diving-fish.com/api/maimaidxprober/chart_stats",
                30,
                None,
                false,
            ),
            (
                EndpointId::DxRatingAliases,
                CatalogSource::DxRatingAliases,
                "https://miruku.dxrating.net/api/v1/aliases",
                15,
                Some("Mozilla/5.0 maimai-bot"),
                false,
            ),
            (
                EndpointId::DxRatingTags,
                CatalogSource::DxRatingTags,
                "https://miruku.dxrating.net/api/v1/tags",
                15,
                Some("Mozilla/5.0 maimai-bot"),
                false,
            ),
            (
                EndpointId::YuzuPlate,
                CatalogSource::Plate,
                "https://www.yuzuchan.moe/api/maimaidx/maimaidxplate",
                10,
                Some("maimai-bot plate-updater"),
                false,
            ),
            (
                EndpointId::WahlapLocations,
                CatalogSource::Location,
                "https://sega-register.wahlap.net/api/sega/maidx/rest/location",
                20,
                Some("maimai-bot location-updater"),
                false,
            ),
        ];
        let endpoints = definitions
            .into_iter()
            .map(|(id, source, url, timeout, ua, text)| {
                EndpointSpec::parse(id, source, url, timeout, ua, text)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { endpoints })
    }

    pub(crate) fn get(&self, id: EndpointId) -> Result<&EndpointSpec, CatalogSourceError> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.id == id)
            .ok_or_else(|| {
                CatalogSourceError::configuration(
                    CatalogSourceErrorCode::InvalidConfiguration,
                    "required source endpoint is missing",
                )
            })
    }

    #[cfg(test)]
    pub(crate) fn local(base: &Url) -> Result<Self, CatalogSourceError> {
        validate_endpoint_url(base)?;
        let definitions = [
            (
                EndpointId::LxnsSongs,
                CatalogSource::Lxns,
                "lxns/songs",
                30,
                None,
                false,
            ),
            (
                EndpointId::LxnsAliases,
                CatalogSource::Lxns,
                "lxns/aliases",
                30,
                None,
                false,
            ),
            (
                EndpointId::DxDataProxy,
                CatalogSource::DxData,
                "dxdata/proxy",
                30,
                Some("maimai-bot dxdata-updater"),
                true,
            ),
            (
                EndpointId::DxDataGithub,
                CatalogSource::DxData,
                "dxdata/github",
                30,
                Some("maimai-bot dxdata-updater"),
                true,
            ),
            (
                EndpointId::DivingFishMusic,
                CatalogSource::DivingFish,
                "divingfish/music",
                30,
                None,
                false,
            ),
            (
                EndpointId::YuzuAliases,
                CatalogSource::Yuzu,
                "yuzu/aliases",
                5,
                Some("maimai-bot yuzu-alias-updater"),
                false,
            ),
            (
                EndpointId::DivingFishChartStats,
                CatalogSource::ChartStats,
                "divingfish/stats",
                30,
                None,
                false,
            ),
            (
                EndpointId::DxRatingAliases,
                CatalogSource::DxRatingAliases,
                "dxrating/aliases",
                15,
                Some("Mozilla/5.0 maimai-bot"),
                false,
            ),
            (
                EndpointId::DxRatingTags,
                CatalogSource::DxRatingTags,
                "dxrating/tags",
                15,
                Some("Mozilla/5.0 maimai-bot"),
                false,
            ),
            (
                EndpointId::YuzuPlate,
                CatalogSource::Plate,
                "yuzu/plate",
                10,
                Some("maimai-bot plate-updater"),
                false,
            ),
            (
                EndpointId::WahlapLocations,
                CatalogSource::Location,
                "wahlap/locations",
                20,
                Some("maimai-bot location-updater"),
                false,
            ),
        ];
        let mut endpoints = Vec::with_capacity(definitions.len());
        for (id, source, path, timeout, user_agent, accepts_text) in definitions {
            let url = base.join(path).map_err(|_| invalid_endpoint())?;
            validate_endpoint_url(&url)?;
            endpoints.push(EndpointSpec {
                id,
                source,
                url,
                timeout: Duration::from_secs(timeout),
                user_agent,
                accepts_text,
            });
        }
        Ok(Self { endpoints })
    }
}

fn validate_endpoint_url(url: &Url) -> Result<(), CatalogSourceError> {
    let valid_scheme = matches!(url.scheme(), "http" | "https");
    let has_credentials = !url.username().is_empty() || url.password().is_some();
    if !valid_scheme
        || url.host_str().is_none()
        || has_credentials
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid_endpoint());
    }
    Ok(())
}

const fn invalid_endpoint() -> CatalogSourceError {
    CatalogSourceError::configuration(
        CatalogSourceErrorCode::InvalidConfiguration,
        "source endpoint must be an HTTP URL without credentials, query, or fragment",
    )
}
