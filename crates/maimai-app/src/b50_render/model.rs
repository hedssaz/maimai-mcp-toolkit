use std::{path::PathBuf, time::Duration};

use maimai_core::ScoreSource;
use time::OffsetDateTime;

use crate::{
    b50_image::B50ImageStyle,
    score_service::{B50Mode, ScoreQuery},
    scores::Lookup,
};

pub struct B50RenderRequest {
    pub lookup: Lookup,
    pub source: Option<ScoreSource>,
    pub mode: B50Mode,
    pub style: B50ImageStyle,
    pub title: Option<String>,
    pub static_dir: Option<PathBuf>,
    pub cover_cache_dir: Option<PathBuf>,
    pub now: OffsetDateTime,
}

impl B50RenderRequest {
    pub const fn new(lookup: Lookup, now: OffsetDateTime) -> Self {
        Self {
            lookup,
            source: None,
            mode: B50Mode::Provider,
            style: B50ImageStyle::Yuzu,
            title: None,
            static_dir: None,
            cover_cache_dir: None,
            now,
        }
    }

    pub(crate) fn score_request(&self) -> crate::score_service::B50Request {
        crate::score_service::B50Request {
            query: ScoreQuery {
                lookup: self.lookup.clone(),
                source: self.source,
                diving_fish_credentials: None,
                now: self.now.unix_timestamp(),
            },
            mode: self.mode,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct B50RenderTimings {
    pub query: Duration,
    pub prepare: Duration,
    pub draw: Duration,
    pub save: Duration,
}

#[derive(Debug)]
pub struct RenderedB50 {
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub source: ScoreSource,
    pub local_computed_at: Option<OffsetDateTime>,
    pub timings: B50RenderTimings,
}
