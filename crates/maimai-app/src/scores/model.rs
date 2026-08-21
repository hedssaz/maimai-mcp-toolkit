use maimai_core::{
    ChartConstant, ChartGeneration, ChartKey, Difficulty, FullComboStatus, FullSyncStatus,
    PlayAchievement, PlayerSelector, PlayerUsername, QqId, RatingBreakdown, ScoreSource,
    SourceSongId,
};
use num_rational::Ratio;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Lookup {
    Qq(QqId),
    Username(PlayerUsername),
}

impl TryFrom<PlayerSelector> for Lookup {
    type Error = super::ScoreError;

    fn try_from(value: PlayerSelector) -> Result<Self, Self::Error> {
        match value {
            PlayerSelector::Qq(value) => Ok(Self::Qq(value)),
            PlayerSelector::Username(value) => Ok(Self::Username(value)),
            PlayerSelector::Auto(_) => Err(super::ScoreError::invalid(
                "成绩业务层不接受未解析的 auto selector",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionReason {
    ExplicitOverride,
    QqPreference,
    Default,
    UsernameFixed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSelection {
    pub preferred_source: ScoreSource,
    pub source: ScoreSource,
    pub reason: SelectionReason,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlayerScoreProfile {
    pub nickname: Option<String>,
    pub username: Option<String>,
    pub rating: Option<u32>,
    pub actual_rating: Option<u32>,
    pub additional_rating: Option<u32>,
    pub plate: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FitLabel {
    Inflated,
    Deflated,
    Equal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B50Chart {
    pub key: ChartKey,
    pub source_song_id: SourceSongId,
    pub title: String,
    pub level: String,
    pub constant: Option<ChartConstant>,
    pub achievements: Option<PlayAchievement>,
    pub dx_score: Option<u32>,
    pub rating: Option<u32>,
    pub original_rating: Option<u32>,
    pub grade: Option<String>,
    pub full_combo: Option<FullComboStatus>,
    pub full_sync: Option<FullSyncStatus>,
    pub version: String,
    pub is_current: bool,
    pub fit_constant: Option<ChartConstant>,
    pub fit_label: Option<FitLabel>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerScores {
    pub lookup: Lookup,
    pub source: ScoreSource,
    pub player: PlayerScoreProfile,
    pub records: Vec<B50Chart>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RatingMode {
    Actual,
    Fit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct B50Computation {
    pub input: usize,
    pub eligible: usize,
    pub duplicate_lower_rating: usize,
    pub skipped_utage: usize,
    pub skipped_missing_rating: usize,
    pub skipped_missing_fit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B50Result {
    pub lookup: Lookup,
    pub source: ScoreSource,
    pub player: PlayerScoreProfile,
    pub rating_breakdown: RatingBreakdown,
    pub b35: Vec<B50Chart>,
    pub b15: Vec<B50Chart>,
    pub mode: RatingMode,
    pub computation: Option<B50Computation>,
    pub fit_index: FitIndex,
}

impl B50Result {
    pub fn total_count(&self) -> usize {
        self.b35.len() + self.b15.len()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExactRatio(Ratio<i128>);

impl ExactRatio {
    pub fn new(numerator: i128, denominator: u128) -> Option<Self> {
        let denominator = i128::try_from(denominator).ok()?;
        (denominator != 0).then(|| Self(Ratio::new(numerator, denominator)))
    }

    pub const fn numerator(&self) -> i128 {
        *self.0.numer()
    }

    pub const fn denominator(&self) -> u128 {
        self.0.denom().unsigned_abs()
    }

    pub(crate) const fn from_ratio(value: Ratio<i128>) -> Self {
        Self(value)
    }

    pub(crate) const fn ratio(&self) -> &Ratio<i128> {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FitIndexLabel {
    ClearlyInflated,
    SlightlyInflated,
    Balanced,
    SlightlyDeflated,
    ClearlyDeflated,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FitIndexSection {
    pub virtual_rating: Option<i64>,
    pub virtual_ratio_percent: Option<ExactRatio>,
    pub weighted_average_delta: Option<ExactRatio>,
    pub counted: usize,
    pub missing: usize,
    pub total_rating: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FitIndex {
    pub label: Option<FitIndexLabel>,
    pub b50: FitIndexSection,
    pub b35: FitIndexSection,
    pub b15: FitIndexSection,
}

impl FitIndex {
    pub const fn available(self) -> bool {
        self.b50.counted > 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongFilter {
    pub song: SourceSongId,
    pub generation: Option<SongGenerationFilter>,
    pub difficulty: Option<Difficulty>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SongGenerationFilter {
    Exact(ChartGeneration),
    UtageAny,
}

impl SongGenerationFilter {
    pub fn matches(self, value: ChartGeneration) -> bool {
        match self {
            Self::Exact(expected) => value == expected,
            Self::UtageAny => matches!(
                value,
                ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
            ),
        }
    }
}

impl SongFilter {
    pub const fn new(song: SourceSongId) -> Self {
        Self {
            song,
            generation: None,
            difficulty: None,
        }
    }
}
