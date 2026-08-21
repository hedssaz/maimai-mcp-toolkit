use crate::RenderError;

const MAX_DOCUMENT_LINES: usize = 64;
const MAX_LINE_CHARACTERS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingRankingDocument {
    lines: Vec<String>,
}

impl RatingRankingDocument {
    pub fn new(lines: Vec<String>) -> Result<Self, RenderError> {
        if lines.is_empty() || lines.len() > MAX_DOCUMENT_LINES {
            return Err(RenderError::invalid(
                "rating_ranking.lines",
                "must contain between 1 and 64 lines",
            ));
        }
        for line in &lines {
            if line.chars().any(char::is_control) || line.chars().count() > MAX_LINE_CHARACTERS {
                return Err(RenderError::invalid(
                    "rating_ranking.line",
                    "must contain at most 256 non-control characters",
                ));
            }
        }
        Ok(Self { lines })
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingRankingRenderedPng {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
