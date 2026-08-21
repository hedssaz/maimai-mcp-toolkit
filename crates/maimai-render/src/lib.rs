//! 只接收 render-ready 数据的纯图片渲染库。

mod assets;
mod completion;
mod cover;
mod deluxe_score;
mod error;
mod layout;
mod maibot;
mod model;
mod music_global_stats;
mod music_info;
mod music_score;
mod rating_ranking;
mod region_heatmap;
mod renderer;
mod rise_score;
mod score_list;
mod template;
mod text;
mod yuzu;

pub use assets::LegacyAssets;
pub use completion::{
    CompletionRenderedPng, CompletionRenderer, CompletionState, FullComboStatus, FullSyncStatus,
    LevelProgressView, PlateChartState, PlateMemberView, PlateTableKind, PlateTableView,
    ProgressPage, RatingAllClear, RatingConstantGroup, RatingScoreCell, RatingStatistics,
    RatingTableMode, RatingTableView, ScoreCardCell,
};
pub use cover::CoverResolver;
pub use error::RenderError;
pub(crate) use error::TemplateStyle;
pub use maibot::MaibotRenderer;
pub use maimai_core::{AchievementRate, ChartConstant, RatingBreakdown, SourceSongId};
pub use model::{
    B50View, ChartType, Difficulty, MissingCover, MissingCoverReason, PlayerHeader, RenderMetadata,
    RenderedPng, ScoreCard, ScoreSection,
};
pub use music_global_stats::{
    MusicGlobalStatsRenderedPng, MusicGlobalStatsRenderer, MusicGlobalStatsView,
};
pub use music_info::{
    MusicInfoChart, MusicInfoRenderedPng, MusicInfoRenderer, MusicInfoView, PlayerSongScoreContext,
};
pub use music_score::{MusicScoreRenderedPng, MusicScoreRenderer, MusicScoreRow, MusicScoreView};
pub use rating_ranking::{RatingRankingDocument, RatingRankingRenderedPng, RatingRankingRenderer};
pub use region_heatmap::{
    RegionHeatmapAssets, RegionHeatmapRenderedPng, RegionHeatmapRenderer, RegionHeatmapRow,
    RegionHeatmapView,
};
pub use renderer::LegacyRenderer;
pub use rise_score::{
    RiseScoreCandidate, RiseScoreRenderError, RiseScoreRenderedPng, RiseScoreRenderer,
    RiseScoreSection, RiseScoreView,
};
pub use score_list::{
    SCORE_LIST_PAGE_SIZE, ScoreListItem, ScoreListRenderError, ScoreListRenderedPng,
    ScoreListRenderer, ScoreListView,
};
pub use yuzu::YuzuRenderer;
