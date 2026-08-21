use maimai_core::ScoreSource;
use maimai_providers::DivingFishCredentials;
use maimai_providers::RawJsonPayload;
use time::OffsetDateTime;

use crate::scores::{Lookup, RatingMode, SongFilter, SourceSelection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerPresentationTrophyColor {
    Normal,
    Bronze,
    Silver,
    Gold,
    Rainbow,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlayerPresentationProfile {
    pub nickname: Option<String>,
    pub rating: Option<u32>,
    pub course_rank: Option<u32>,
    pub class_rank: Option<u32>,
    pub star: Option<u32>,
    pub trophy_id: Option<u32>,
    pub trophy_name: Option<String>,
    pub trophy_color: Option<PlayerPresentationTrophyColor>,
    pub icon_id: Option<u32>,
    pub plate_id: Option<u32>,
    pub frame_id: Option<u32>,
    pub upload_time: Option<OffsetDateTime>,
}

pub struct ScoreQuery {
    pub lookup: Lookup,
    pub source: Option<ScoreSource>,
    pub diving_fish_credentials: Option<DivingFishCredentials>,
    pub now: i64,
}

impl ScoreQuery {
    pub const fn new(lookup: Lookup, now: i64) -> Self {
        Self {
            lookup,
            source: None,
            diving_fish_credentials: None,
            now,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B50Mode {
    Provider,
    Computed(RatingMode),
}

pub struct B50Request {
    pub query: ScoreQuery,
    pub mode: B50Mode,
}

pub struct SongScoresRequest {
    pub query: ScoreQuery,
    pub filter: SongFilter,
}

#[derive(Debug)]
pub struct ScoreServiceResponse<T> {
    result: T,
    raw: Option<RawJsonPayload>,
    selection: SourceSelection,
}

impl<T> ScoreServiceResponse<T> {
    pub(crate) fn new(result: T, raw: Option<RawJsonPayload>, selection: SourceSelection) -> Self {
        Self {
            result,
            raw,
            selection,
        }
    }

    pub fn result(&self) -> &T {
        &self.result
    }

    pub fn into_result(self) -> T {
        self.result
    }

    pub const fn selection(&self) -> SourceSelection {
        self.selection
    }

    pub fn into_parts(self) -> (T, Option<RawJsonPayload>, SourceSelection) {
        (self.result, self.raw, self.selection)
    }
}
