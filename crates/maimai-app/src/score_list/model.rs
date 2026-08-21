use std::path::PathBuf;

use maimai_core::{ChartConstant, ScoreSource};
use time::OffsetDateTime;

use crate::scores::Lookup;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoreListTarget {
    Level(String),
    Constant(ChartConstant),
}

impl ScoreListTarget {
    pub fn level(value: impl Into<String>) -> Result<Self, super::ScoreListError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_control) || value.chars().count() > 16 {
            return Err(super::ScoreListError::invalid(
                "level/rating 必须是非空等级字符串",
            ));
        }
        Ok(Self::Level(value.to_owned()))
    }

    pub const fn constant(value: ChartConstant) -> Self {
        Self::Constant(value)
    }

    pub fn label(&self) -> String {
        match self {
            Self::Level(value) => value.clone(),
            Self::Constant(value) => {
                let mut value = value.value().normalize().to_string();
                if !value.contains('.') {
                    value.push_str(".0");
                }
                value
            }
        }
    }
}

pub struct ScoreListRequest {
    pub lookup: Lookup,
    pub source: Option<ScoreSource>,
    pub target: ScoreListTarget,
    pub page: usize,
    pub now: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreListImage {
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub source: ScoreSource,
    pub placeholder_covers: usize,
}
