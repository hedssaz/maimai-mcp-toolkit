pub(crate) mod assets;
pub(crate) mod card;
pub(crate) mod layout;
mod model;
mod renderer;

use thiserror::Error;

pub use model::{SCORE_LIST_PAGE_SIZE, ScoreListItem, ScoreListRenderedPng, ScoreListView};
pub use renderer::ScoreListRenderer;

#[derive(Debug, Error)]
pub enum ScoreListRenderError {
    #[error("missing score-list assets: {missing:?}")]
    AssetsRequired { missing: Vec<String> },
    #[error("invalid score-list asset: {name}")]
    InvalidAsset { name: String },
    #[error(transparent)]
    Render(#[from] crate::RenderError),
}

impl ScoreListRenderError {
    pub const fn assets_required(&self) -> bool {
        matches!(
            self,
            Self::AssetsRequired { .. } | Self::InvalidAsset { .. }
        )
    }
}
