pub(crate) mod catalog;
pub(crate) mod identity;
pub(crate) mod oauth;
pub(crate) mod rankings;
pub(crate) mod render;
pub(crate) mod scores;

use maimai_mcp::{
    PairDispatcher,
    score_by_song_tools::{ScoreBySongDispatcher, TOOL_NAME as SCORE_BY_SONG_TOOL},
};
use thiserror::Error;

use super::services::PublicServices;
use crate::lifecycle::{SharedLifecycle, SharedLifecycleError};

const SCORE_BY_SONG_TOOLS: [&str; 1] = [SCORE_BY_SONG_TOOL];

type ScoresTail = PairDispatcher<ScoreBySongDispatcher, scores::ScoresDispatcher>;
type RenderTail = PairDispatcher<render::RenderDispatcher, ScoresTail>;
type RankingsTail = PairDispatcher<maimai_mcp::ranking_tools::RankingDispatcher, RenderTail>;
type OAuthTail = PairDispatcher<maimai_mcp::oauth_tools::OAuthDispatcher, RankingsTail>;
type IdentityTail = PairDispatcher<maimai_mcp::identity_tools::IdentityDispatcher, OAuthTail>;
pub type PublicDispatcher =
    PairDispatcher<maimai_mcp::catalog_tools::CatalogDispatcher, IdentityTail>;

pub(super) struct PublicComposition {
    pub dispatcher: PublicDispatcher,
    pub lifecycle: PublicLifecycle,
}

pub(super) type PublicLifecycle = SharedLifecycle;

pub(super) fn compose(services: PublicServices) -> Result<PublicComposition, CompositionError> {
    let dispatcher = PairDispatcher::new(
        &catalog::TOOL_NAMES,
        catalog::dispatcher(&services),
        PairDispatcher::new(
            &identity::TOOL_NAMES,
            identity::dispatcher(&services)?,
            PairDispatcher::new(
                &oauth::TOOL_NAMES,
                oauth::dispatcher(&services),
                PairDispatcher::new(
                    &rankings::TOOL_NAMES,
                    rankings::dispatcher(&services),
                    PairDispatcher::new(
                        &render::TOOL_NAMES,
                        render::dispatcher(&services)?,
                        PairDispatcher::new(
                            &SCORE_BY_SONG_TOOLS,
                            ScoreBySongDispatcher::new(services.score_by_song.clone()),
                            scores::dispatcher(&services),
                        ),
                    ),
                ),
            ),
        ),
    );
    let lifecycle = SharedLifecycle::new(
        services.rankings,
        services.identity,
        services.catalog_jobs,
        services.state,
    );
    Ok(PublicComposition {
        dispatcher,
        lifecycle,
    })
}

#[derive(Debug, Error)]
pub enum CompositionError {
    #[error(transparent)]
    Identity(#[from] maimai_app::identity::IdentityError),
    #[error(transparent)]
    Completion(#[from] maimai_mcp::render_tools::completion::CompletionToolError),
}

pub(super) type LifecycleError = SharedLifecycleError;

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, error::Error};

    use super::*;

    #[test]
    fn surface_partitions_are_disjoint_and_cover_public_contract() -> Result<(), Box<dyn Error>> {
        let partitions: [&[&str]; 7] = [
            &catalog::TOOL_NAMES,
            &identity::TOOL_NAMES,
            &oauth::TOOL_NAMES,
            &rankings::TOOL_NAMES,
            &render::TOOL_NAMES,
            &SCORE_BY_SONG_TOOLS,
            &scores::PUBLIC_TOOL_NAMES,
        ];
        let flattened = partitions
            .into_iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let unique = flattened.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(flattened.len(), 60);
        assert_eq!(unique.len(), 60);
        let fragments = [
            include_str!("../../../../../contracts/public/catalog.json"),
            include_str!("../../../../../contracts/public/identity.json"),
            include_str!("../../../../../contracts/public/oauth.json"),
            include_str!("../../../../../contracts/public/rankings.json"),
            include_str!("../../../../../contracts/public/render.json"),
            include_str!("../../../../../contracts/public/score_query.json"),
            include_str!("../../../../../contracts/public/scores.json"),
        ];
        let contract = maimai_mcp::contract::SurfaceContract::compose(
            "maimai-public",
            env!("CARGO_PKG_VERSION"),
            &fragments,
        )?;
        assert_eq!(
            flattened,
            contract
                .tools()
                .iter()
                .map(|tool| tool.name())
                .collect::<Vec<_>>()
        );
        Ok(())
    }
}
