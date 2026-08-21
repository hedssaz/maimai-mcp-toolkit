use std::{io, path::Path};

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemplateStyle {
    Yuzu,
    Maibot,
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("invalid render model field {field}: {message}")]
    InvalidModel {
        field: &'static str,
        message: String,
    },
    #[error("failed to read {kind} asset {path}")]
    AssetRead {
        kind: &'static str,
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("invalid {kind} asset {path}")]
    InvalidAsset { kind: &'static str, path: String },
    #[error("failed to encode PNG")]
    PngEncode(#[source] image::ImageError),

    #[error("required {style} template assets are missing")]
    AssetsRequired {
        style: &'static str,
        missing: Vec<String>,
    },
}

impl RenderError {
    pub(crate) fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidModel {
            field,
            message: message.into(),
        }
    }

    pub(crate) fn asset_read(kind: &'static str, path: &Path, source: io::Error) -> Self {
        Self::AssetRead {
            kind,
            path: path.display().to_string(),
            source,
        }
    }

    pub(crate) fn invalid_asset(kind: &'static str, path: &Path) -> Self {
        Self::InvalidAsset {
            kind,
            path: path.display().to_string(),
        }
    }

    pub(crate) fn assets_required(style: TemplateStyle, missing: Vec<String>) -> Self {
        Self::AssetsRequired {
            style: match style {
                TemplateStyle::Yuzu => "yuzu",
                TemplateStyle::Maibot => "maibot",
            },
            missing,
        }
    }
}
