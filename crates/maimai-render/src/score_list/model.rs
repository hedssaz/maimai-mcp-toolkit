use maimai_core::{
    ChartConstant, ChartGeneration, Difficulty, FullComboStatus, FullSyncStatus, PlayAchievement,
    SongIdValue,
};

use crate::RenderError;

pub const SCORE_LIST_PAGE_SIZE: usize = 80;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreListItem {
    pub display_id: SongIdValue,
    pub cover_id: SongIdValue,
    pub image_name: Option<String>,
    pub title: String,
    pub generation: ChartGeneration,
    pub difficulty: Difficulty,
    pub constant: Option<ChartConstant>,
    pub achievement: Option<PlayAchievement>,
    pub dx_score: Option<u32>,
    pub max_dx_score: Option<u32>,
    pub rating: Option<u32>,
    pub combo: Option<FullComboStatus>,
    pub sync: Option<FullSyncStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreListView {
    pub target: String,
    pub page: usize,
    pub pages: usize,
    pub total: usize,
    pub first: usize,
    pub items: Vec<ScoreListItem>,
}

impl ScoreListView {
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.target.trim().is_empty()
            || self.target.chars().any(char::is_control)
            || self.target.chars().count() > 32
        {
            return Err(RenderError::invalid(
                "score_list.target",
                "must contain 1 to 32 non-control characters",
            ));
        }
        if self.page == 0 || self.pages == 0 || self.page > self.pages {
            return Err(RenderError::invalid(
                "score_list.page",
                "must be inside the available page range",
            ));
        }
        if self.items.len() > SCORE_LIST_PAGE_SIZE {
            return Err(RenderError::invalid(
                "score_list.items",
                "contains more than 80 records",
            ));
        }
        for item in &self.items {
            if item.title.chars().any(char::is_control) || item.title.chars().count() > 256 {
                return Err(RenderError::invalid(
                    "score_list.item.title",
                    "must contain at most 256 non-control characters",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreListRenderedPng {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub placeholder_covers: usize,
}
