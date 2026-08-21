use super::super::ExactCount;
use super::{DisplayMode, JudgmentCounts, NoteTotals, NoteType, ScoreMode, ShortcutConstraints};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PercentageDetails {
    pub raw: String,
    pub display_floor: String,
    pub display_half_up: String,
    pub scaled_floor: i128,
    pub scaled_half_up: i128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectedTotal {
    Raw(i128),
    Percentage(PercentageDetails),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreTotals {
    pub base: i128,
    pub break_bonus: i128,
    pub old_score: i128,
    pub dx_score: i128,
    pub old_achievement: Option<PercentageDetails>,
    pub dx_achievement: Option<PercentageDetails>,
    pub selected_mode: Option<SelectedTotal>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerNoteScores {
    pub base: i128,
    pub break_bonus: i128,
    pub old_score: i128,
    pub dx_score: i128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreRow {
    pub note_type: NoteType,
    pub judgment: String,
    pub count: u32,
    pub per_note: PerNoteScores,
    pub contribution: PerNoteScores,
    pub selected_contribution: Option<i128>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreCountResult {
    pub display_digits: u32,
    pub note_totals: NoteTotals,
    pub rows: Vec<ScoreRow>,
    pub totals: ScoreTotals,
    pub score_mode: Option<ScoreMode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetRange {
    Raw {
        min_score: i128,
        max_score: i128,
    },
    Percentage {
        min: String,
        max: Option<String>,
        max_exclusive: Option<String>,
        mode_detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricDetails {
    pub max_base: i128,
    pub max_break_bonus: Option<i128>,
    pub max_old: Option<i128>,
    pub denominator: i128,
    pub max_metric: i128,
    pub base_weight: Option<i128>,
    pub break_bonus_weight: Option<i128>,
    pub min_loss: i128,
    pub max_loss: i128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerTypeSummary {
    pub note_type: NoteType,
    pub possible_metric_count: usize,
    pub combination_count: ExactCount,
    pub sample_combination_count: usize,
    pub min_possible_metric: i128,
    pub max_possible_metric: i128,
    pub allowed_judgments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchSolution {
    pub metric: i128,
    pub loss_metric: Option<i128>,
    pub percentage: Option<PercentageDetails>,
    pub counts: JudgmentCounts,
    pub totals: ScoreTotals,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub found: bool,
    pub score_mode: ScoreMode,
    pub display_mode: Option<DisplayMode>,
    pub display_digits: Option<u32>,
    pub note_totals: NoteTotals,
    pub target_range: TargetRange,
    pub metric_details: Option<MetricDetails>,
    pub matching_metric_count: usize,
    pub matching_combination_count: ExactCount,
    pub returned_solution_count: usize,
    pub truncated: bool,
    pub solutions: Vec<SearchSolution>,
    pub per_type_summary: Vec<PerTypeSummary>,
    pub shortcut_constraints: ShortcutConstraints,
}
