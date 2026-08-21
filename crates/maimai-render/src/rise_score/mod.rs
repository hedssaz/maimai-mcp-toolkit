mod assets;
mod card;
mod layout;
mod model;
mod renderer;

use thiserror::Error;

pub use model::{RiseScoreCandidate, RiseScoreRenderedPng, RiseScoreSection, RiseScoreView};
pub use renderer::RiseScoreRenderer;

#[derive(Debug, Error)]
pub enum RiseScoreRenderError {
    #[error("missing rise-score assets: {missing:?}")]
    AssetsRequired { missing: Vec<String> },
    #[error("invalid rise-score asset: {name}")]
    InvalidAsset { name: String },
    #[error(transparent)]
    Render(#[from] crate::RenderError),
}

impl RiseScoreRenderError {
    pub const fn assets_required(&self) -> bool {
        matches!(
            self,
            Self::AssetsRequired { .. } | Self::InvalidAsset { .. }
        )
    }
}

#[cfg(test)]
mod tests;
