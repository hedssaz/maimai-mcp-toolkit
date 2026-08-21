use std::{fmt, str::FromStr};

use super::super::ScoringError;

pub const NOTE_TYPE_COUNT: usize = 5;
pub const JUDGMENT_COUNT: usize = 8;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NoteType {
    Tap,
    Touch,
    Hold,
    Slide,
    Break,
}

impl NoteType {
    pub const ALL: [Self; NOTE_TYPE_COUNT] =
        [Self::Tap, Self::Touch, Self::Hold, Self::Slide, Self::Break];

    pub const fn index(self) -> usize {
        match self {
            Self::Tap => 0,
            Self::Touch => 1,
            Self::Hold => 2,
            Self::Slide => 3,
            Self::Break => 4,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tap => "tap",
            Self::Touch => "touch",
            Self::Hold => "hold",
            Self::Slide => "slide",
            Self::Break => "break",
        }
    }
}

impl fmt::Display for NoteType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for NoteType {
    type Err = ScoringError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match clean_name(value).as_str() {
            "tap" => Ok(Self::Tap),
            "touch" | "tch" | "touch_note" => Ok(Self::Touch),
            "hold" | "touchhold" | "touch_hold" | "touchh" | "touch_hold_note" | "thold" => {
                Ok(Self::Hold)
            }
            "slide" => Ok(Self::Slide),
            "break" | "brk" | "break_note" => Ok(Self::Break),
            _ => Err(ScoringError::UnknownNoteType(value.to_owned())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Judgment {
    Miss,
    Good,
    GreatLow,
    GreatMid,
    GreatHigh,
    PerfectLow,
    PerfectHigh,
    Critical,
}

impl Judgment {
    pub const ALL: [Self; JUDGMENT_COUNT] = [
        Self::Miss,
        Self::Good,
        Self::GreatLow,
        Self::GreatMid,
        Self::GreatHigh,
        Self::PerfectLow,
        Self::PerfectHigh,
        Self::Critical,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Miss => 0,
            Self::Good => 1,
            Self::GreatLow => 2,
            Self::GreatMid => 3,
            Self::GreatHigh => 4,
            Self::PerfectLow => 5,
            Self::PerfectHigh => 6,
            Self::Critical => 7,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Miss => "miss",
            Self::Good => "good",
            Self::GreatLow => "great_low",
            Self::GreatMid => "great_mid",
            Self::GreatHigh => "great_high",
            Self::PerfectLow => "perfect_low",
            Self::PerfectHigh => "perfect_high",
            Self::Critical => "critical",
        }
    }

    pub fn parse_count(value: &str, note_type: NoteType) -> Result<Self, ScoringError> {
        match clean_name(value).as_str() {
            "m" | "miss" => Ok(Self::Miss),
            "g" | "good" => Ok(Self::Good),
            "great" if note_type == NoteType::Break => {
                Err(ScoringError::AmbiguousBreakJudgment("great"))
            }
            "great" => Ok(Self::GreatMid),
            "great_low" => Ok(Self::GreatLow),
            "great_mid" => Ok(Self::GreatMid),
            "great_high" => Ok(Self::GreatHigh),
            "perfect" if note_type == NoteType::Break => {
                Err(ScoringError::AmbiguousBreakJudgment("perfect"))
            }
            "perfect" => Ok(Self::PerfectHigh),
            "perfect_low" => Ok(Self::PerfectLow),
            "perfect_high" => Ok(Self::PerfectHigh),
            "critical" | "critical_perfect" | "cp" => Ok(Self::Critical),
            _ => Err(ScoringError::UnknownJudgment(value.to_owned())),
        }
    }

    pub const fn display_name(self, note_type: NoteType) -> &'static str {
        if !matches!(note_type, NoteType::Break) {
            if matches!(self, Self::GreatLow | Self::GreatMid | Self::GreatHigh) {
                return "great";
            }
            if matches!(self, Self::PerfectLow | Self::PerfectHigh) {
                return "perfect";
            }
        }
        self.as_str()
    }
}

impl fmt::Display for Judgment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JudgmentSet(u16);

impl JudgmentSet {
    pub const EMPTY: Self = Self(0);
    pub const ALL: Self = Self((1 << JUDGMENT_COUNT) - 1);

    pub const fn singleton(judgment: Judgment) -> Self {
        Self(1 << judgment.index())
    }

    pub fn insert(&mut self, judgment: Judgment) {
        self.0 |= 1 << judgment.index();
    }

    pub fn remove(&mut self, judgment: Judgment) {
        self.0 &= !(1 << judgment.index());
    }

    pub const fn contains(self, judgment: Judgment) -> bool {
        self.0 & (1 << judgment.index()) != 0
    }

    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    pub fn iter(self) -> impl Iterator<Item = Judgment> {
        Judgment::ALL
            .into_iter()
            .filter(move |judgment| self.contains(*judgment))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JudgmentGroup {
    All,
    NotMiss,
    Great,
    Perfect,
    PerfectOrCritical,
    FullComboPlus,
    FullCombo,
}

impl JudgmentGroup {
    pub fn judgments(self) -> JudgmentSet {
        let mut result = JudgmentSet::EMPTY;
        match self {
            Self::All => JudgmentSet::ALL,
            Self::NotMiss | Self::FullCombo => {
                for judgment in Judgment::ALL {
                    if judgment != Judgment::Miss {
                        result.insert(judgment);
                    }
                }
                result
            }
            Self::Great => {
                result.insert(Judgment::GreatLow);
                result.insert(Judgment::GreatMid);
                result.insert(Judgment::GreatHigh);
                result
            }
            Self::Perfect => {
                result.insert(Judgment::PerfectLow);
                result.insert(Judgment::PerfectHigh);
                result
            }
            Self::PerfectOrCritical => {
                result.insert(Judgment::PerfectLow);
                result.insert(Judgment::PerfectHigh);
                result.insert(Judgment::Critical);
                result
            }
            Self::FullComboPlus => {
                for judgment in Judgment::ALL {
                    if !matches!(judgment, Judgment::Miss | Judgment::Good) {
                        result.insert(judgment);
                    }
                }
                result
            }
        }
    }
}

impl FromStr for JudgmentGroup {
    type Err = ScoringError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match clean_name(value).as_str() {
            "all" | "any" => Ok(Self::All),
            "not_miss" | "no_miss" => Ok(Self::NotMiss),
            "great" => Ok(Self::Great),
            "perfect" => Ok(Self::Perfect),
            "perfect_or_critical" | "ap" => Ok(Self::PerfectOrCritical),
            "fc_plus" => Ok(Self::FullComboPlus),
            "fc" => Ok(Self::FullCombo),
            _ => Err(ScoringError::UnknownJudgment(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteTotals([u32; NOTE_TYPE_COUNT]);

impl NoteTotals {
    pub const fn new(tap: u32, touch: u32, hold: u32, slide: u32, break_notes: u32) -> Self {
        Self([tap, touch, hold, slide, break_notes])
    }

    pub const fn get(&self, note_type: NoteType) -> u32 {
        self.0[note_type.index()]
    }

    pub fn set(&mut self, note_type: NoteType, value: u32) {
        self.0[note_type.index()] = value;
    }

    pub fn total(&self) -> u64 {
        self.0.iter().copied().map(u64::from).sum()
    }
}

impl Default for NoteTotals {
    fn default() -> Self {
        Self([0; NOTE_TYPE_COUNT])
    }
}

impl From<crate::NoteCounts> for NoteTotals {
    fn from(value: crate::NoteCounts) -> Self {
        Self::new(
            value.tap,
            value.touch,
            value.hold,
            value.slide,
            value.break_notes,
        )
    }
}

impl From<NoteTotals> for crate::NoteCounts {
    fn from(value: NoteTotals) -> Self {
        Self {
            tap: value.get(NoteType::Tap),
            hold: value.get(NoteType::Hold),
            slide: value.get(NoteType::Slide),
            touch: value.get(NoteType::Touch),
            break_notes: value.get(NoteType::Break),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JudgmentCounts([[u32; JUDGMENT_COUNT]; NOTE_TYPE_COUNT]);

impl JudgmentCounts {
    pub const fn empty() -> Self {
        Self([[0; JUDGMENT_COUNT]; NOTE_TYPE_COUNT])
    }

    pub const fn get(&self, note_type: NoteType, judgment: Judgment) -> u32 {
        self.0[note_type.index()][judgment.index()]
    }

    pub fn set(&mut self, note_type: NoteType, judgment: Judgment, value: u32) {
        self.0[note_type.index()][judgment.index()] = value;
    }

    pub fn add(
        &mut self,
        note_type: NoteType,
        judgment: Judgment,
        value: u32,
    ) -> Result<(), ScoringError> {
        let current = self.get(note_type, judgment);
        self.set(
            note_type,
            judgment,
            current
                .checked_add(value)
                .ok_or(ScoringError::Arithmetic("judgment count overflow"))?,
        );
        Ok(())
    }

    pub fn note_total(&self, note_type: NoteType) -> Result<u32, ScoringError> {
        self.0[note_type.index()]
            .iter()
            .copied()
            .try_fold(0_u32, |sum, count| {
                sum.checked_add(count)
                    .ok_or(ScoringError::Arithmetic("note total overflow"))
            })
    }
}

impl Default for JudgmentCounts {
    fn default() -> Self {
        Self::empty()
    }
}

pub(super) fn clean_name(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}
