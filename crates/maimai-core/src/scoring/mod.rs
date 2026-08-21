mod calculate;
mod count;
mod rational;
mod search;
mod types;

use std::{error::Error, fmt};

pub use calculate::score_counts;
pub use count::ExactCount;
pub use search::find_score_combinations;
pub use types::{
    DisplayMode, Judgment, JudgmentCounts, JudgmentGroup, JudgmentSet, MetricDetails, NoteTotals,
    NoteType, OptionalJudgmentCounts, PerNoteScores, PerTypeSummary, PercentageDetails,
    PercentageInput, ScoreCountRequest, ScoreCountResult, ScoreMode, ScoreRow, ScoreTotals,
    SearchConstraints, SearchRequest, SearchResult, SearchSolution, SearchTarget, SelectedTotal,
    ShortcutConstraints, TargetRange,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoringError {
    UnknownNoteType(String),
    UnknownJudgment(String),
    AmbiguousBreakJudgment(&'static str),
    UnknownScoreMode(String),
    InvalidDisplayDigits(u32),
    InvalidDecimal {
        field: &'static str,
    },
    InvalidInput(String),
    Constraint(String),
    StateLimit {
        note_type: Option<NoteType>,
        max_states: usize,
    },
    Arithmetic(&'static str),
}

impl fmt::Display for ScoringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNoteType(value) => write!(formatter, "unknown note type: {value:?}"),
            Self::UnknownJudgment(value) => write!(formatter, "unknown judgment: {value:?}"),
            Self::AmbiguousBreakJudgment(value) => write!(
                formatter,
                "break.{value} is ambiguous; use an explicit low/mid/high judgment"
            ),
            Self::UnknownScoreMode(value) => write!(formatter, "unknown score_mode: {value:?}"),
            Self::InvalidDisplayDigits(value) => {
                write!(
                    formatter,
                    "display_digits must be between 1 and 8, got {value}"
                )
            }
            Self::InvalidDecimal { field } => write!(formatter, "{field} must be a decimal number"),
            Self::InvalidInput(message) | Self::Constraint(message) => formatter.write_str(message),
            Self::StateLimit {
                note_type: Some(note_type),
                max_states,
            } => write!(
                formatter,
                "search exceeded max_states={max_states} while solving {note_type}"
            ),
            Self::StateLimit {
                note_type: None,
                max_states,
            } => write!(
                formatter,
                "combined search exceeded max_states={max_states}"
            ),
            Self::Arithmetic(message) => write!(formatter, "scoring arithmetic error: {message}"),
        }
    }
}

impl Error for ScoringError {}
