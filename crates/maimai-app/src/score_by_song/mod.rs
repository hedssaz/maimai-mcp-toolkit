mod error;
mod model;
mod resolve;
mod service;

use std::sync::Arc;

use maimai_catalog::CatalogStore;

use crate::{identity::IdentityDirectory, score_service::PlayerScoreService};

pub use error::{ScoreBySongError, ScoreBySongErrorCode};
pub use model::{
    MusicIdScores, PlayerLookupRequest, ScoreBySongRequest, ScoreBySongResult, SelectedSong,
    SongCandidate, SongLookupRequest, SongSelection,
};

#[derive(Clone)]
pub struct ScoreBySongService {
    identities: IdentityDirectory,
    catalog: Arc<CatalogStore>,
    scores: Arc<PlayerScoreService>,
}

impl ScoreBySongService {
    pub fn new(
        identities: IdentityDirectory,
        catalog: Arc<CatalogStore>,
        scores: Arc<PlayerScoreService>,
    ) -> Self {
        Self {
            identities,
            catalog,
            scores,
        }
    }
}

#[cfg(test)]
mod tests;
