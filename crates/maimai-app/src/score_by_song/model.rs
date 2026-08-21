use std::collections::BTreeSet;

use maimai_core::{
    ChartGeneration, Difficulty, GroupId, PlayerUsername, QqId, ScoreSource, SourceSongId,
};
use maimai_providers::RawJsonPayload;
use maimai_storage::IdentityRecord;
use time::OffsetDateTime;

use crate::scores::{B50Chart, Lookup, PlayerScoreProfile, SongGenerationFilter, SourceSelection};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlayerLookupRequest {
    Qq(QqId),
    Username(PlayerUsername),
    Auto(PlayerUsername),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongLookupRequest {
    pub query: String,
    pub difficulty: Option<Difficulty>,
    pub generation: Option<SongGenerationFilter>,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreBySongRequest {
    pub player: PlayerLookupRequest,
    pub group_id: Option<GroupId>,
    pub song: SongLookupRequest,
    pub include_raw: bool,
    pub now: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongCandidate {
    pub id: SourceSongId,
    pub title: String,
    pub artist: String,
    pub source: &'static str,
    pub available_generations: BTreeSet<ChartGeneration>,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedSong {
    pub candidate: SongCandidate,
    pub music_ids: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongSelection {
    pub auto_selected: bool,
    pub selected_rank: usize,
    pub total_matches: usize,
    pub truncated: bool,
    pub candidates: Vec<SongCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicIdScores {
    pub music_id: u32,
    pub records: Vec<B50Chart>,
}

#[derive(Debug)]
pub struct ScoreBySongResult {
    pub requested_at: OffsetDateTime,
    pub requested_player: PlayerLookupRequest,
    pub lookup: Lookup,
    pub identity: Option<IdentityRecord>,
    pub song_query: String,
    pub selected_song: SelectedSong,
    pub selection: SongSelection,
    pub source: ScoreSource,
    pub source_selection: SourceSelection,
    pub player: PlayerScoreProfile,
    pub scores: Vec<MusicIdScores>,
    pub raw: Option<RawJsonPayload>,
}
