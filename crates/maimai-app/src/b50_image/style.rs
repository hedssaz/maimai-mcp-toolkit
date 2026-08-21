use std::{fmt, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{
    B50ImageError,
    atomic_file::{atomic_write, reject_non_regular, secure_absolute},
    utc_timestamp,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum B50ImageStyle {
    Legacy,
    #[default]
    Yuzu,
    Maibot,
}

impl fmt::Display for B50ImageStyle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Legacy => "legacy",
            Self::Yuzu => "yuzu",
            Self::Maibot => "maibot",
        })
    }
}

#[derive(Clone, Debug)]
pub struct StyleStore {
    path: PathBuf,
    fallback: B50ImageStyle,
    default_override: Option<B50ImageStyle>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleSelection {
    pub style: B50ImageStyle,
    pub config_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleUpdate {
    pub style: B50ImageStyle,
    pub config_path: PathBuf,
    pub updated_at: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StyleDocument {
    style: B50ImageStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

impl StyleStore {
    pub fn new(path: impl Into<PathBuf>, fallback: B50ImageStyle) -> Result<Self, B50ImageError> {
        let path = secure_absolute(path.into())?;
        Ok(Self {
            path,
            fallback,
            default_override: None,
        })
    }

    pub fn with_default_override(mut self, style: Option<B50ImageStyle>) -> Self {
        self.default_override = style;
        self
    }

    pub fn current(&self) -> Result<StyleSelection, B50ImageError> {
        let style = match self.default_override {
            Some(style) => style,
            None if !reject_non_regular(&self.path)? => self.fallback,
            None => self.read_document()?.style,
        };
        Ok(StyleSelection {
            style,
            config_path: self.path.clone(),
        })
    }

    pub fn set(
        &self,
        style: B50ImageStyle,
        now: OffsetDateTime,
    ) -> Result<StyleUpdate, B50ImageError> {
        let updated_at = utc_timestamp(now);
        let document = StyleDocument {
            style,
            updated_at: Some(updated_at.clone()),
        };
        let mut bytes = serde_json::to_vec_pretty(&document).map_err(|_| {
            B50ImageError::InvalidStyleConfig {
                path: self.path.clone(),
            }
        })?;
        bytes.push(b'\n');
        atomic_write(&self.path, &bytes)?;
        Ok(StyleUpdate {
            style,
            config_path: self.path.clone(),
            updated_at,
        })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn read_document(&self) -> Result<StyleDocument, B50ImageError> {
        let bytes = fs::read(&self.path).map_err(|source| B50ImageError::io(&self.path, source))?;
        serde_json::from_slice(&bytes).map_err(|_| B50ImageError::InvalidStyleConfig {
            path: self.path.clone(),
        })
    }
}
