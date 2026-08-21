use maimai_mcp::{
    PairDispatcher,
    render_tools::{
        MusicInfoDispatcher,
        b50::{B50RenderDispatcher, RenderDeployment},
        completion::{CompletionDispatcher, CompletionSurface, CompletionToolError},
        global_stats::MusicGlobalStatsDispatcher,
        music_score::MusicScoreDispatcher,
        rating_ranking::RatingRankingDispatcher,
        rise_score::{RiseScoreDispatcher, RiseScoreSurface},
        score_list::{ScoreListDispatcher, ScoreListSurface},
    },
};
use time::OffsetDateTime;

use crate::public::services::PublicServices;

const B50_TOOLS: [&str; 1] = ["render_maimai_b50"];
const COMPLETION_TOOLS: [&str; 6] = maimai_mcp::render_tools::completion::TOOL_NAMES;
const MUSIC_INFO_TOOLS: [&str; 2] = maimai_mcp::render_tools::TOOL_NAMES;
const MUSIC_SCORE_TOOLS: [&str; 1] = ["render_maimai_music_score"];
const GLOBAL_STATS_TOOLS: [&str; 1] = ["render_maimai_music_global_stats"];
const RISE_SCORE_TOOLS: [&str; 1] = ["render_maimai_rise_score"];
const SCORE_LIST_TOOLS: [&str; 1] = ["render_maimai_score_list"];
pub const TOOL_NAMES: [&str; 14] = [
    "render_maimai_b50",
    "render_maimai_plate",
    "render_maimai_plate_batch",
    "render_maimai_rating",
    "render_maimai_progress",
    "render_maimai_music_info",
    "render_maimai_music_info_batch",
    "render_maimai_music_score",
    "render_maimai_music_global_stats",
    "render_maimai_rise_score",
    "render_maimai_score_list",
    "render_maimai_rating_ranking",
    "render_maimai_plate_progress",
    "render_maimai_plate_progress_batch",
];

type ScoreListTail = PairDispatcher<ScoreListDispatcher, RatingRankingDispatcher>;
type RiseTail = PairDispatcher<RiseScoreDispatcher, ScoreListTail>;
type GlobalTail = PairDispatcher<MusicGlobalStatsDispatcher, RiseTail>;
type MusicScoreTail = PairDispatcher<MusicScoreDispatcher, GlobalTail>;
type MusicInfoTail = PairDispatcher<MusicInfoDispatcher, MusicScoreTail>;
type CompletionTail = PairDispatcher<CompletionDispatcher, MusicInfoTail>;
pub type RenderDispatcher = PairDispatcher<B50RenderDispatcher, CompletionTail>;

pub fn dispatcher(services: &PublicServices) -> Result<RenderDispatcher, CompletionToolError> {
    let rating_ranking = RatingRankingDispatcher::new(
        services.render.rating_ranking.clone(),
        OffsetDateTime::now_utc,
    );
    Ok(PairDispatcher::new(
        &B50_TOOLS,
        B50RenderDispatcher::new(services.render.b50.clone(), RenderDeployment::Public),
        PairDispatcher::new(
            &COMPLETION_TOOLS,
            CompletionDispatcher::new(
                services.render.completion.clone(),
                CompletionSurface::Public,
            )?,
            PairDispatcher::new(
                &MUSIC_INFO_TOOLS,
                MusicInfoDispatcher::new(services.render.music_info.clone()),
                PairDispatcher::new(
                    &MUSIC_SCORE_TOOLS,
                    MusicScoreDispatcher::public(services.render.music_score.clone()),
                    PairDispatcher::new(
                        &GLOBAL_STATS_TOOLS,
                        MusicGlobalStatsDispatcher::new(services.render.global_stats.clone()),
                        PairDispatcher::new(
                            &RISE_SCORE_TOOLS,
                            RiseScoreDispatcher::new(
                                services.render.rise_score.clone(),
                                RiseScoreSurface::Public,
                                OffsetDateTime::now_utc,
                            ),
                            PairDispatcher::new(
                                &SCORE_LIST_TOOLS,
                                ScoreListDispatcher::new(
                                    services.render.score_list.clone(),
                                    ScoreListSurface::Public,
                                    OffsetDateTime::now_utc,
                                ),
                                rating_ranking,
                            ),
                        ),
                    ),
                ),
            ),
        ),
    ))
}
