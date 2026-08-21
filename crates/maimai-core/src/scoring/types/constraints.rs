use super::{JUDGMENT_COUNT, Judgment, JudgmentSet, NOTE_TYPE_COUNT, NoteType};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionalJudgmentCounts([[Option<u32>; JUDGMENT_COUNT]; NOTE_TYPE_COUNT]);

impl OptionalJudgmentCounts {
    pub const fn empty() -> Self {
        Self([[None; JUDGMENT_COUNT]; NOTE_TYPE_COUNT])
    }

    pub const fn get(&self, note_type: NoteType, judgment: Judgment) -> Option<u32> {
        self.0[note_type.index()][judgment.index()]
    }

    pub fn set(&mut self, note_type: NoteType, judgment: Judgment, value: Option<u32>) {
        self.0[note_type.index()][judgment.index()] = value;
    }
}

impl Default for OptionalJudgmentCounts {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchConstraints {
    pub allowed: [JudgmentSet; NOTE_TYPE_COUNT],
    pub disallowed: [JudgmentSet; NOTE_TYPE_COUNT],
    pub fixed: OptionalJudgmentCounts,
    pub minimum: OptionalJudgmentCounts,
    pub maximum: OptionalJudgmentCounts,
    pub no_miss_good: bool,
    pub break_max_non_critical: Option<u32>,
}

impl Default for SearchConstraints {
    fn default() -> Self {
        Self {
            allowed: [JudgmentSet::ALL; NOTE_TYPE_COUNT],
            disallowed: [JudgmentSet::EMPTY; NOTE_TYPE_COUNT],
            fixed: OptionalJudgmentCounts::empty(),
            minimum: OptionalJudgmentCounts::empty(),
            maximum: OptionalJudgmentCounts::empty(),
            no_miss_good: false,
            break_max_non_critical: None,
        }
    }
}

impl SearchConstraints {
    pub fn allow_only(&mut self, note_type: NoteType, judgments: JudgmentSet) {
        self.allowed[note_type.index()] = judgments;
    }

    pub fn disallow(&mut self, note_type: NoteType, judgments: JudgmentSet) {
        self.disallowed[note_type.index()] = judgments;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutConstraints {
    pub no_miss_good: bool,
    pub break_max_perfect_or_below: Option<u32>,
    pub break_min_critical: Option<u32>,
}
