use maimai_core::single_song_rating;
use num_rational::Ratio;

use super::{B50Chart, ExactRatio, FitIndex, FitIndexLabel, FitIndexSection};

pub fn compute_fit_index(b35: &[B50Chart], b15: &[B50Chart]) -> FitIndex {
    let b50 = section(b35.iter().chain(b15));
    let b35 = section(b35.iter());
    let b15 = section(b15.iter());
    FitIndex {
        label: b50.virtual_ratio_percent.as_ref().map(label),
        b50,
        b35,
        b15,
    }
}

fn section<'a>(charts: impl IntoIterator<Item = &'a B50Chart>) -> FitIndexSection {
    let mut virtual_rating = 0i64;
    let mut total_rating = 0u64;
    let mut weighted_delta = Ratio::from_integer(0i128);
    let mut counted = 0usize;
    let mut missing = 0usize;
    for chart in charts {
        let actual_rating = chart.original_rating.or(chart.rating);
        let values = actual_rating
            .zip(chart.constant)
            .zip(chart.fit_constant)
            .zip(chart.achievements.and_then(|value| value.ranked()));
        let Some((((actual_rating, constant), fit_constant), achievements)) = values else {
            missing += 1;
            continue;
        };
        if constant.value().mantissa() == 0 {
            missing += 1;
            continue;
        }
        let Ok(fitted_rating) = single_song_rating(fit_constant, achievements) else {
            missing += 1;
            continue;
        };
        virtual_rating += i64::from(actual_rating) - i64::from(fitted_rating);
        total_rating += u64::from(actual_rating);
        let delta = constant.value() - fit_constant.value();
        let exact_delta = Ratio::new(delta.mantissa(), 10i128.pow(delta.scale()));
        weighted_delta += exact_delta * i128::from(actual_rating);
        counted += 1;
    }
    let virtual_ratio_percent = (counted > 0 && total_rating > 0).then(|| {
        ExactRatio::from_ratio(Ratio::new(
            i128::from(virtual_rating) * 100,
            i128::from(total_rating),
        ))
    });
    let weighted_average_delta = (counted > 0 && total_rating > 0)
        .then(|| ExactRatio::from_ratio(weighted_delta / i128::from(total_rating)));
    FitIndexSection {
        virtual_rating: (counted > 0).then_some(virtual_rating),
        virtual_ratio_percent,
        weighted_average_delta,
        counted,
        missing,
        total_rating: (counted > 0).then_some(total_rating),
    }
}

fn label(value: &ExactRatio) -> FitIndexLabel {
    let ratio = value.ratio();
    if ratio > &Ratio::from_integer(1) {
        FitIndexLabel::ClearlyInflated
    } else if ratio > &Ratio::new(1, 5) {
        FitIndexLabel::SlightlyInflated
    } else if ratio < &Ratio::from_integer(-1) {
        FitIndexLabel::ClearlyDeflated
    } else if ratio < &Ratio::new(-1, 5) {
        FitIndexLabel::SlightlyDeflated
    } else {
        FitIndexLabel::Balanced
    }
}
