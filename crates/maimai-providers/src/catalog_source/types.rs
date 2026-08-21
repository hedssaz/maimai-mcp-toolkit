use std::fmt::Write as _;

use reqwest::header::HeaderValue;

use super::error::{CatalogSourceError, CatalogSourceErrorCode};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CatalogSource {
    Lxns,
    DxData,
    DivingFish,
    Yuzu,
    ChartStats,
    DxRatingAliases,
    DxRatingTags,
    Plate,
    Location,
}

impl CatalogSource {
    pub const ALL: [Self; 9] = [
        Self::Lxns,
        Self::DxData,
        Self::DivingFish,
        Self::Yuzu,
        Self::ChartStats,
        Self::DxRatingAliases,
        Self::DxRatingTags,
        Self::Plate,
        Self::Location,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Lxns => "lxns",
            Self::DxData => "dxdata",
            Self::DivingFish => "divingfish",
            Self::Yuzu => "yuzu",
            Self::ChartStats => "chart_stats",
            Self::DxRatingAliases => "dxrating_aliases",
            Self::DxRatingTags => "dxrating_tags",
            Self::Plate => "plate",
            Self::Location => "location",
        }
    }

    pub const fn targets(self) -> &'static [SourceTarget] {
        match self {
            Self::Lxns => &[SourceTarget::LxnsSongList, SourceTarget::LxnsAliasList],
            Self::DxData => &[SourceTarget::DxData],
            Self::DivingFish => &[SourceTarget::DivingFishSongList],
            Self::Yuzu => &[SourceTarget::YuzuAliasList],
            Self::ChartStats => &[SourceTarget::DivingFishChartStats],
            Self::DxRatingAliases => &[SourceTarget::DxRatingAliases],
            Self::DxRatingTags => &[SourceTarget::DxRatingTags],
            Self::Plate => &[SourceTarget::Plate],
            Self::Location => &[SourceTarget::Location],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceTarget {
    LxnsSongList,
    LxnsAliasList,
    DxData,
    DivingFishSongList,
    YuzuAliasList,
    DivingFishChartStats,
    DxRatingAliases,
    DxRatingTags,
    Plate,
    Location,
}

impl SourceTarget {
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::LxnsSongList => "lxns_song_list.json",
            Self::LxnsAliasList => "lxns_alias_list.json",
            Self::DxData => "dxdata.json",
            Self::DivingFishSongList => "divingfish_song_list.json",
            Self::YuzuAliasList => "music_alias.json",
            Self::DivingFishChartStats => "divingfish_chart_stats.json",
            Self::DxRatingAliases => "dxrating_aliases.json",
            Self::DxRatingTags => "dxrating_tags.json",
            Self::Plate => "maimaidxplate.json",
            Self::Location => "sega_maidx_locations.json",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BundleStatus {
    Updated,
    NotModified,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentStatistics {
    Records {
        records: usize,
    },
    ChartStats {
        charts: usize,
        difficulty_buckets: usize,
    },
    Tags {
        tags: usize,
        groups: usize,
        associations: usize,
    },
    Plates {
        plates: usize,
    },
    Locations {
        locations: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentDigest([u8; 32]);

impl DocumentDigest {
    pub(crate) const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            let _ = write!(output, "{byte:02x}");
        }
        output
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDocument {
    target: SourceTarget,
    bytes: Vec<u8>,
    digest: DocumentDigest,
    statistics: DocumentStatistics,
    content_type: Option<String>,
}

impl SourceDocument {
    pub(crate) fn new(
        target: SourceTarget,
        bytes: Vec<u8>,
        digest: DocumentDigest,
        statistics: DocumentStatistics,
        content_type: Option<String>,
    ) -> Self {
        Self {
            target,
            bytes,
            digest,
            statistics,
            content_type,
        }
    }

    pub const fn target(&self) -> SourceTarget {
        self.target
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn digest(&self) -> DocumentDigest {
        self.digest
    }

    pub const fn statistics(&self) -> &DocumentStatistics {
        &self.statistics
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMetadata {
    update_time: Option<String>,
}

impl SourceMetadata {
    pub(crate) fn empty() -> Self {
        Self { update_time: None }
    }

    pub(crate) fn with_update_time(update_time: Option<String>) -> Self {
        Self { update_time }
    }

    pub fn update_time(&self) -> Option<&str> {
        self.update_time.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBundle {
    source: CatalogSource,
    status: BundleStatus,
    documents: Vec<SourceDocument>,
    etag: Option<EntityTag>,
    metadata: SourceMetadata,
}

impl SourceBundle {
    pub(crate) fn updated(
        source: CatalogSource,
        documents: Vec<SourceDocument>,
        etag: Option<EntityTag>,
        metadata: SourceMetadata,
    ) -> Self {
        Self {
            source,
            status: BundleStatus::Updated,
            documents,
            etag,
            metadata,
        }
    }

    pub(crate) fn not_modified(source: CatalogSource, etag: Option<EntityTag>) -> Self {
        Self {
            source,
            status: BundleStatus::NotModified,
            documents: Vec::new(),
            etag,
            metadata: SourceMetadata::empty(),
        }
    }

    pub const fn source(&self) -> CatalogSource {
        self.source
    }

    pub const fn status(&self) -> BundleStatus {
        self.status
    }

    pub fn documents(&self) -> &[SourceDocument] {
        &self.documents
    }

    pub fn etag(&self) -> Option<&EntityTag> {
        self.etag.as_ref()
    }

    pub const fn metadata(&self) -> &SourceMetadata {
        &self.metadata
    }

    pub fn into_documents(self) -> Vec<SourceDocument> {
        self.documents
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityTag(String);

impl EntityTag {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, CatalogSourceError> {
        let value = value.as_ref().trim();
        if value.is_empty() || value.len() > 512 || !value.is_ascii() {
            return Err(CatalogSourceError::configuration(
                CatalogSourceErrorCode::InvalidEntityTag,
                "entity tag must contain 1 to 512 ASCII bytes",
            ));
        }
        let (weak, opaque) = if let Some(value) = value.strip_prefix("W/") {
            (true, value)
        } else {
            (false, value)
        };
        let opaque = if opaque.starts_with('"') || opaque.ends_with('"') {
            opaque
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .ok_or_else(invalid_entity_tag)?
        } else {
            opaque
        };
        if opaque.is_empty()
            || opaque
                .bytes()
                .any(|byte| byte != 0x21 && !(0x23..=0x7e).contains(&byte))
        {
            return Err(invalid_entity_tag());
        }
        let header = if weak {
            format!("W/\"{opaque}\"")
        } else {
            format!("\"{opaque}\"")
        };
        HeaderValue::from_str(&header).map_err(|_| {
            CatalogSourceError::configuration(
                CatalogSourceErrorCode::InvalidEntityTag,
                "entity tag is not a valid HTTP header value",
            )
        })?;
        Ok(Self(header))
    }

    pub fn as_header_value(&self) -> &str {
        &self.0
    }
}

const fn invalid_entity_tag() -> CatalogSourceError {
    CatalogSourceError::configuration(
        CatalogSourceErrorCode::InvalidEntityTag,
        "entity tag does not use valid HTTP entity-tag syntax",
    )
}
