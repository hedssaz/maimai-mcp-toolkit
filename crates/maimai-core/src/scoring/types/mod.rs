mod constraints;
mod note;
mod request;
mod result;

pub use constraints::{OptionalJudgmentCounts, SearchConstraints, ShortcutConstraints};
pub use note::{Judgment, JudgmentCounts, JudgmentGroup, JudgmentSet, NoteTotals, NoteType};
pub use request::{
    DisplayMode, PercentageInput, ScoreCountRequest, ScoreMode, SearchRequest, SearchTarget,
};
pub use result::{
    MetricDetails, PerNoteScores, PerTypeSummary, PercentageDetails, ScoreCountResult, ScoreRow,
    ScoreTotals, SearchResult, SearchSolution, SelectedTotal, TargetRange,
};

pub(super) use note::{JUDGMENT_COUNT, NOTE_TYPE_COUNT};
