use std::{error::Error, fs};

use time::OffsetDateTime;

use crate::b50_image::B50ImageStyle;

use super::ResourceOverridePolicy;
use super::service::local_computed_at;

#[test]
fn whitespace_nickname_is_a_present_player() -> Result<(), Box<dyn Error>> {
    use crate::scores::{B50Result, FitIndex, Lookup, PlayerScoreProfile, RatingMode};
    use maimai_core::{QqId, RatingBreakdown, ScoreSource};

    for nickname in ["", "　", " Name \t\n"] {
        let result = B50Result {
            lookup: Lookup::Qq(QqId::new("10001")?),
            source: ScoreSource::DivingFish,
            player: PlayerScoreProfile {
                nickname: Some(nickname.to_owned()),
                ..Default::default()
            },
            rating_breakdown: RatingBreakdown {
                b35: 0,
                b15: 0,
                total: 0,
            },
            b35: vec![],
            b15: vec![],
            mode: RatingMode::Actual,
            computation: None,
            fit_index: FitIndex::default(),
        };
        let view = super::view::prepare(&result, None)?;
        assert_eq!(view.player().nickname(), nickname);
    }
    Ok(())
}

#[test]
fn resource_override_policy_allows_canonical_descendants_only() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let static_root = temporary.path().join("static");
    let static_child = static_root.join("theme");
    let cover_root = temporary.path().join("covers");
    let outside = temporary.path().join("outside");
    for path in [&static_root, &static_child, &cover_root, &outside] {
        fs::create_dir_all(path)?;
    }
    let policy = ResourceOverridePolicy::new(
        static_root.clone(),
        vec![static_root.clone()],
        vec![cover_root.clone()],
    )?;
    assert_eq!(
        policy.static_dir(Some(static_child.clone()), B50ImageStyle::Yuzu)?,
        Some(static_child.canonicalize()?)
    );
    assert_eq!(
        policy.cover_cache_dir(Some(cover_root.clone()))?,
        Some(cover_root.canonicalize()?)
    );
    assert!(
        policy
            .static_dir(Some(outside), B50ImageStyle::Yuzu)
            .is_err()
    );
    Ok(())
}

#[test]
fn legacy_runtime_static_root_must_equal_the_loaded_default() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let default_static = temporary.path().join("default-static");
    let other_static = temporary.path().join("other-static");
    let covers = temporary.path().join("covers");
    for path in [&default_static, &other_static] {
        fs::create_dir_all(path.join("mai"))?;
    }
    fs::create_dir(&covers)?;
    let policy =
        ResourceOverridePolicy::new(default_static, vec![other_static.clone()], vec![covers])?;
    let error = policy
        .static_dir(Some(other_static), B50ImageStyle::Legacy)
        .err()
        .ok_or("legacy override should fail")?;
    assert!(matches!(
        error,
        super::B50RenderError::LegacyStaticOverrideUnsupported
    ));
    Ok(())
}

#[test]
fn caption_timestamp_is_only_evidence_from_a_local_b50_computation() {
    let now = OffsetDateTime::UNIX_EPOCH;
    assert_eq!(
        local_computed_at(maimai_core::ScoreSource::Local, now),
        Some(now)
    );
    assert_eq!(
        local_computed_at(maimai_core::ScoreSource::DivingFish, now),
        None
    );
    assert_eq!(local_computed_at(maimai_core::ScoreSource::Lxns, now), None);
}
