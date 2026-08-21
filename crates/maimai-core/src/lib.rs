//! 与传输、数据库、HTTP 和绘图框架无关的舞萌领域类型与规则。

mod achievement;
mod catalog;
mod chart;
mod error;
mod identity;
mod player;
mod rating;
mod score_marker;
pub mod scoring;
mod today;

pub use achievement::{
    AchievementRate, PlayAchievement, PlayAchievementError, PlayAchievementKind, UtageScore,
};
pub use catalog::{Chart, Music, NoteCounts};
pub use chart::{
    ChartGeneration, ChartKey, Difficulty, SongIdNamespace, SongIdText, SongIdValue, SourceSongId,
};
pub use error::ValidationError;
pub use identity::{GroupId, QqId};
pub use player::{PlayerSelector, PlayerUsername, ScoreSource};
pub use rating::{
    AchievementRank, ChartConstant, RatingBreakdown, RatingError, achievement_rank,
    b50_rating_breakdown, coefficient_tenths, maimai_dx_ra, single_song_rating, top_rating_sum,
};
pub use score_marker::{FullComboStatus, FullSyncStatus, ScoreMarkerParseError};
pub use today::{
    ACTIVITIES, REMINDER, TodayDate, TodayError, TodayResult, TodaySong, format_today_maimai,
    qq_hash, qqhash, today_maimai,
};
