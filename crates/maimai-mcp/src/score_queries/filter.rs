mod parse;

use std::cmp::Ordering;

use maimai_app::scores::{B50Chart, B50Result, FitLabel, PlayerScores};
use maimai_core::{AchievementRate, ChartConstant, Difficulty};
use rust_decimal::Decimal;

use super::{convert::parse_difficulty, dto::B50Args, error::ScoreQueryToolError};
use parse::{decimal, decimal_value, fit_label, integer, section, sort_key, sort_order};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Section {
    B50,
    B35,
    B15,
    Split,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortKey {
    Default,
    Rating,
    Achievement,
    Constant,
    FitConstant,
    FitDelta,
    Title,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

#[derive(Clone, Debug)]
pub struct DisplayOptions {
    pub top_n: usize,
    pub section: Section,
    pub include_chart_metadata: bool,
    pub sort_key: SortKey,
    pub sort_order: SortOrder,
    pub level: Option<String>,
    pub difficulty: Option<Difficulty>,
    pub constant: Range<ChartConstant>,
    pub achievement: Range<AchievementRate>,
    pub rating: Range<u32>,
    pub fit_constant: Range<ChartConstant>,
    pub fit_delta: Range<Decimal>,
    pub fit_label: Option<FitLabel>,
}

#[derive(Clone, Debug, Default)]
pub struct Range<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

impl DisplayOptions {
    pub fn from_args(args: &B50Args) -> Result<Self, ScoreQueryToolError> {
        let top_n = args.top_n.unwrap_or(50);
        if top_n > 50 {
            return Err(ScoreQueryToolError::invalid(
                "topN 必须是 0 到 50 之间的整数。",
            ));
        }
        let options = Self {
            top_n: usize::try_from(top_n)
                .map_err(|_| ScoreQueryToolError::invalid("topN 超出范围。"))?,
            section: section(args.section.as_deref())?,
            include_chart_metadata: args.include_chart_metadata != Some(false),
            sort_key: sort_key(args.sort_by.as_deref())?,
            sort_order: sort_order(args.sort_order.as_deref())?,
            level: args
                .level
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned),
            difficulty: args.difficulty.clone().map(parse_difficulty).transpose()?,
            constant: Range::new(
                decimal(
                    args.ds_min.as_ref(),
                    "dsMin",
                    ChartConstant::from_decimal_str,
                )?,
                decimal(
                    args.ds_max.as_ref(),
                    "dsMax",
                    ChartConstant::from_decimal_str,
                )?,
                "ds",
            )?,
            achievement: Range::new(
                decimal(
                    args.achievement_min.as_ref(),
                    "achievementMin",
                    AchievementRate::from_decimal_str,
                )?,
                decimal(
                    args.achievement_max.as_ref(),
                    "achievementMax",
                    AchievementRate::from_decimal_str,
                )?,
                "achievement",
            )?,
            rating: Range::new(
                integer(args.ra_min.as_ref(), "raMin")?,
                integer(args.ra_max.as_ref(), "raMax")?,
                "ra",
            )?,
            fit_constant: Range::new(
                decimal(
                    args.fit_diff_min.as_ref(),
                    "fitDiffMin",
                    ChartConstant::from_decimal_str,
                )?,
                decimal(
                    args.fit_diff_max.as_ref(),
                    "fitDiffMax",
                    ChartConstant::from_decimal_str,
                )?,
                "fitDiff",
            )?,
            fit_delta: Range::new(
                decimal_value(args.fit_delta_min.as_ref(), "fitDeltaMin")?,
                decimal_value(args.fit_delta_max.as_ref(), "fitDeltaMax")?,
                "fitDelta",
            )?,
            fit_label: fit_label(args.fit_label.as_deref())?,
        };
        Ok(options)
    }

    pub fn visible<'a>(&self, result: &'a B50Result) -> Vec<&'a B50Chart> {
        match self.section {
            Section::B35 => self.visible_slice(&result.b35),
            Section::B15 => self.visible_slice(&result.b15),
            Section::B50 | Section::Split => {
                let mut values = result.b35.iter().chain(&result.b15).collect::<Vec<_>>();
                self.filter_sort_truncate(&mut values);
                values
            }
        }
    }

    pub fn visible_b35<'a>(&self, result: &'a B50Result) -> Vec<&'a B50Chart> {
        self.visible_slice(&result.b35)
    }

    pub fn visible_b15<'a>(&self, result: &'a B50Result) -> Vec<&'a B50Chart> {
        self.visible_slice(&result.b15)
    }

    fn visible_slice<'a>(&self, charts: &'a [B50Chart]) -> Vec<&'a B50Chart> {
        let mut values = charts.iter().collect::<Vec<_>>();
        self.filter_sort_truncate(&mut values);
        values
    }

    fn filter_sort_truncate(&self, values: &mut Vec<&B50Chart>) {
        values.retain(|chart| self.matches(chart));
        values.sort_by(|left, right| self.compare(left, right));
        values.truncate(self.top_n);
    }

    fn matches(&self, chart: &B50Chart) -> bool {
        self.level
            .as_ref()
            .is_none_or(|level| normalize_b50_level(&chart.level) == normalize_b50_level(level))
            && self
                .difficulty
                .is_none_or(|difficulty| chart.key.difficulty() == difficulty)
            && self.constant.contains(chart.constant)
            && self
                .achievement
                .contains(chart.achievements.and_then(|value| value.ranked()))
            && self.rating.contains(chart.rating)
            && self.fit_constant.contains(chart.fit_constant)
            && self.fit_delta.contains(
                chart
                    .constant
                    .zip(chart.fit_constant)
                    .map(|(actual, fit)| actual.value() - fit.value()),
            )
            && self
                .fit_label
                .is_none_or(|label| chart.fit_label == Some(label))
    }

    fn compare(&self, left: &B50Chart, right: &B50Chart) -> Ordering {
        match self.sort_key {
            SortKey::Default => default_order(left, right),
            SortKey::Title => match self.sort_order {
                SortOrder::Ascending => left.title.cmp(&right.title),
                SortOrder::Descending => right.title.cmp(&left.title),
            },
            SortKey::Rating => numeric_order(left.rating, right.rating, self.sort_order)
                .then_with(|| default_order(left, right)),
            SortKey::Achievement => numeric_order(
                left.achievements.map(|value| value.ten_thousandths()),
                right.achievements.map(|value| value.ten_thousandths()),
                self.sort_order,
            )
            .then_with(|| default_order(left, right)),
            SortKey::Constant => numeric_order(left.constant, right.constant, self.sort_order)
                .then_with(|| default_order(left, right)),
            SortKey::FitConstant => {
                numeric_order(left.fit_constant, right.fit_constant, self.sort_order)
                    .then_with(|| default_order(left, right))
            }
            SortKey::FitDelta => numeric_order(fit_delta(left), fit_delta(right), self.sort_order)
                .then_with(|| default_order(left, right)),
        }
    }

    pub fn summary(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(level) = &self.level {
            parts.push(format!("等级 {level}"));
        }
        if let Some(difficulty) = self.difficulty {
            parts.push(format!("难度 {}", difficulty_name(difficulty)));
        }
        push_range(&mut parts, "定数", &self.constant, |value| {
            value.value().to_string()
        });
        push_range(&mut parts, "达成率", &self.achievement, |value| {
            let units = value.ten_thousandths();
            format!("{}.{:04}", units / 10_000, units % 10_000)
        });
        push_range(&mut parts, "ra", &self.rating, ToString::to_string);
        push_range(&mut parts, "拟合定数", &self.fit_constant, |value| {
            value.value().to_string()
        });
        push_range(&mut parts, "差值", &self.fit_delta, ToString::to_string);
        if let Some(label) = self.fit_label {
            parts.push(match label {
                FitLabel::Inflated => "虚高".to_owned(),
                FitLabel::Deflated => "虚低".to_owned(),
                FitLabel::Equal => "持平".to_owned(),
            });
        }
        if self.sort_key != SortKey::Default || self.sort_order != SortOrder::Descending {
            parts.push(format!(
                "排序 {} {}",
                sort_name(self.sort_key),
                match self.sort_order {
                    SortOrder::Ascending => "asc",
                    SortOrder::Descending => "desc",
                }
            ));
        }
        (!parts.is_empty()).then(|| parts.join("，"))
    }
}

impl<T: Ord> Range<T> {
    fn new(
        min: Option<T>,
        max: Option<T>,
        field: &'static str,
    ) -> Result<Self, ScoreQueryToolError> {
        if min
            .as_ref()
            .zip(max.as_ref())
            .is_some_and(|(min, max)| min > max)
        {
            return Err(ScoreQueryToolError::invalid(format!(
                "{field} 最小值不能大于最大值。"
            )));
        }
        Ok(Self { min, max })
    }

    fn contains(&self, value: Option<T>) -> bool {
        if self.min.is_none() && self.max.is_none() {
            return true;
        }
        value.is_some_and(|value| {
            self.min.as_ref().is_none_or(|min| &value >= min)
                && self.max.as_ref().is_none_or(|max| &value <= max)
        })
    }
}

pub fn filter_records(
    scores: &PlayerScores,
    level: Option<&str>,
    versions: Option<&[String]>,
) -> Vec<B50Chart> {
    scores
        .records
        .iter()
        .filter(|chart| level.is_none_or(|level| chart.level == level.trim()))
        .filter(|chart| {
            versions.is_none_or(|versions| versions.iter().any(|version| version == &chart.version))
        })
        .cloned()
        .collect()
}

fn fit_delta(chart: &B50Chart) -> Option<Decimal> {
    chart
        .constant
        .zip(chart.fit_constant)
        .map(|(actual, fit)| actual.value() - fit.value())
}

fn normalize_b50_level(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|character| !matches!(character, ' ' | '级' | '?'))
        .collect()
}

fn default_order(left: &B50Chart, right: &B50Chart) -> Ordering {
    numeric_order(left.rating, right.rating, SortOrder::Descending)
        .then_with(|| {
            numeric_order(
                left.achievements.map(|value| value.ten_thousandths()),
                right.achievements.map(|value| value.ten_thousandths()),
                SortOrder::Descending,
            )
        })
        .then_with(|| left.title.cmp(&right.title))
}

fn numeric_order<T: Ord>(left: Option<T>, right: Option<T>, order: SortOrder) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => match order {
            SortOrder::Ascending => left.cmp(&right),
            SortOrder::Descending => right.cmp(&left),
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn push_range<T>(
    parts: &mut Vec<String>,
    label: &str,
    range: &Range<T>,
    render: impl Fn(&T) -> String,
) {
    match (&range.min, &range.max) {
        (Some(min), Some(max)) => parts.push(format!("{label} {}-{}", render(min), render(max))),
        (Some(min), None) => parts.push(format!("{label}>={}", render(min))),
        (None, Some(max)) => parts.push(format!("{label}<={}", render(max))),
        (None, None) => {}
    }
}

const fn difficulty_name(value: Difficulty) -> &'static str {
    match value {
        Difficulty::Basic => "Basic",
        Difficulty::Advanced => "Advanced",
        Difficulty::Expert => "Expert",
        Difficulty::Master => "Master",
        Difficulty::ReMaster => "Re:MASTER",
        Difficulty::Utage => "Utage",
    }
}

const fn sort_name(value: SortKey) -> &'static str {
    match value {
        SortKey::Default => "default",
        SortKey::Rating => "ra",
        SortKey::Achievement => "achievement",
        SortKey::Constant => "ds",
        SortKey::FitConstant => "fitDiff",
        SortKey::FitDelta => "fitDelta",
        SortKey::Title => "title",
    }
}
