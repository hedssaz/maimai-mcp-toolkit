use std::fmt;

use thiserror::Error;

use super::types::CatalogSource;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogSourceErrorCode {
    InvalidConfiguration,
    InvalidEntityTag,
    Timeout,
    Network,
    Redirect,
    HttpStatus,
    BodyTooLarge,
    InvalidContentType,
    InvalidResponseHeader,
    InvalidJson,
    InvalidShape,
    AllEndpointsFailed,
}

impl fmt::Display for CatalogSourceErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfiguration => "invalid_configuration",
            Self::InvalidEntityTag => "invalid_entity_tag",
            Self::Timeout => "timeout",
            Self::Network => "network",
            Self::Redirect => "redirect",
            Self::HttpStatus => "http_status",
            Self::BodyTooLarge => "body_too_large",
            Self::InvalidContentType => "invalid_content_type",
            Self::InvalidResponseHeader => "invalid_response_header",
            Self::InvalidJson => "invalid_json",
            Self::InvalidShape => "invalid_shape",
            Self::AllEndpointsFailed => "all_endpoints_failed",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointId {
    LxnsSongs,
    LxnsAliases,
    DxDataProxy,
    DxDataGithub,
    DivingFishMusic,
    YuzuAliases,
    DivingFishChartStats,
    DxRatingAliases,
    DxRatingTags,
    YuzuPlate,
    WahlapLocations,
}

impl fmt::Display for EndpointId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::LxnsSongs => "lxns_songs",
            Self::LxnsAliases => "lxns_aliases",
            Self::DxDataProxy => "dxdata_proxy",
            Self::DxDataGithub => "dxdata_github",
            Self::DivingFishMusic => "divingfish_music",
            Self::YuzuAliases => "yuzu_aliases",
            Self::DivingFishChartStats => "divingfish_chart_stats",
            Self::DxRatingAliases => "dxrating_aliases",
            Self::DxRatingTags => "dxrating_tags",
            Self::YuzuPlate => "yuzu_plate",
            Self::WahlapLocations => "wahlap_locations",
        })
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("catalog source {code}: {message}")]
pub struct CatalogSourceError {
    code: CatalogSourceErrorCode,
    catalog_source: Option<CatalogSource>,
    endpoint: Option<EndpointId>,
    status: Option<u16>,
    message: &'static str,
}

impl CatalogSourceError {
    pub(crate) const fn configuration(code: CatalogSourceErrorCode, message: &'static str) -> Self {
        Self {
            code,
            catalog_source: None,
            endpoint: None,
            status: None,
            message,
        }
    }

    pub(crate) const fn response(
        code: CatalogSourceErrorCode,
        source: CatalogSource,
        endpoint: EndpointId,
        status: Option<u16>,
        message: &'static str,
    ) -> Self {
        Self {
            code,
            catalog_source: Some(source),
            endpoint: Some(endpoint),
            status,
            message,
        }
    }

    pub(crate) const fn source(
        code: CatalogSourceErrorCode,
        source: CatalogSource,
        message: &'static str,
    ) -> Self {
        Self {
            code,
            catalog_source: Some(source),
            endpoint: None,
            status: None,
            message,
        }
    }

    pub const fn code(&self) -> CatalogSourceErrorCode {
        self.code
    }

    pub const fn source_id(&self) -> Option<CatalogSource> {
        self.catalog_source
    }

    pub const fn endpoint(&self) -> Option<EndpointId> {
        self.endpoint
    }

    pub const fn status(&self) -> Option<u16> {
        self.status
    }

    pub const fn safe_message(&self) -> &'static str {
        self.message
    }
}
