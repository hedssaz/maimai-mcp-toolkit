use maimai_core::ScoreSource;

use super::{Lookup, ScoreError, ScoreErrorCode, SelectionReason, SourceSelection};

pub fn select_source(
    lookup: &Lookup,
    explicit: Option<ScoreSource>,
    qq_preference: Option<ScoreSource>,
) -> Result<SourceSelection, ScoreError> {
    if matches!(lookup, Lookup::Username(_)) {
        return Ok(SourceSelection {
            preferred_source: ScoreSource::DivingFish,
            source: ScoreSource::DivingFish,
            reason: SelectionReason::UsernameFixed,
        });
    }
    let preferred_source = qq_preference.unwrap_or(ScoreSource::DivingFish);
    let (source, reason) = if let Some(source) = explicit {
        (source, SelectionReason::ExplicitOverride)
    } else if qq_preference.is_some() {
        (preferred_source, SelectionReason::QqPreference)
    } else {
        (ScoreSource::DivingFish, SelectionReason::Default)
    };
    if source == ScoreSource::OfficialCn {
        return Err(ScoreError::new(
            ScoreErrorCode::UnsupportedSource,
            "成绩查询不支持 official_cn 来源；官方成绩只用于本地导入",
        ));
    }
    Ok(SourceSelection {
        preferred_source,
        source,
        reason,
    })
}
