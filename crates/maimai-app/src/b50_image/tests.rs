use std::{
    collections::BTreeSet,
    error::Error,
    fs, io,
    sync::{Arc, Barrier},
    time::SystemTime,
};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, symlink};

use time::{Duration, OffsetDateTime};

use maimai_core::RatingBreakdown;
use maimai_render::{B50View, LegacyAssets, LegacyRenderer, PlayerHeader};

use super::{
    B50ImageError, B50ImageService, B50ImageStyle, OutputPolicy, OutputStore, RenderOptions,
    ResourceDirectories, StyleStore,
};

fn instant() -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp(1_768_435_200)
}

#[test]
fn style_store_is_atomic_private_and_compatible() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("state").join("style.json");
    let store = StyleStore::new(&path, B50ImageStyle::Yuzu)?;
    assert_eq!(store.current()?.style, B50ImageStyle::Yuzu);

    let updated = store.set(B50ImageStyle::Legacy, instant()?)?;
    assert_eq!(updated.style, B50ImageStyle::Legacy);
    assert_eq!(store.current()?.style, B50ImageStyle::Legacy);
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    assert_eq!(
        document,
        serde_json::json!({
            "style": "legacy",
            "updatedAt": "2026-01-15T00:00:00.000000+00:00",
        })
    );
    #[cfg(unix)]
    {
        assert_eq!(fs::metadata(&path)?.mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(path.parent().ok_or("style parent missing")?)?.mode() & 0o777,
            0o700
        );
    }

    store.set(B50ImageStyle::Maibot, instant()?)?;
    assert_eq!(store.current()?.style, B50ImageStyle::Maibot);
    fs::write(&path, br#"{"style":"legacy"}"#)?;
    assert_eq!(store.current()?.style, B50ImageStyle::Legacy);
    let overridden = store
        .clone()
        .with_default_override(Some(B50ImageStyle::Yuzu));
    assert_eq!(overridden.current()?.style, B50ImageStyle::Yuzu);
    Ok(())
}

#[cfg(unix)]
#[test]
fn style_and_output_stores_reject_symlinks_and_non_files() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let target = temporary.path().join("target.json");
    fs::write(&target, br#"{"style":"legacy"}"#)?;
    let link = temporary.path().join("style.json");
    symlink(&target, &link)?;
    let style_error = StyleStore::new(&link, B50ImageStyle::Yuzu)
        .err()
        .ok_or("expected symlink style error")?;
    assert!(matches!(style_error, B50ImageError::UnsafePath { .. }));

    let output_link = temporary.path().join("output");
    symlink(temporary.path(), &output_link)?;
    let output_error = OutputStore::new(&output_link, OutputPolicy::standard())
        .err()
        .ok_or("expected symlink output error")?;
    assert!(matches!(output_error, B50ImageError::UnsafePath { .. }));

    let directory_config = temporary.path().join("directory-config");
    fs::create_dir(&directory_config)?;
    assert!(matches!(
        StyleStore::new(&directory_config, B50ImageStyle::Yuzu)?.current(),
        Err(B50ImageError::UnsafePath { .. })
    ));
    Ok(())
}

#[test]
fn output_store_sanitizes_atomically_and_cleans_by_ttl_and_count() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("nested").join("images");
    let policy = OutputPolicy::new(Duration::days(1), 2)?;
    let store = OutputStore::new(&root, policy)?;
    let png = b"\x89PNG\r\n\x1a\nfixture";

    let collision_store = Arc::new(OutputStore::new(
        temporary.path().join("collisions"),
        OutputPolicy::new(Duration::days(1), 10)?,
    )?);
    let barrier = Arc::new(Barrier::new(8));
    let collision_now = instant()?;
    let mut handles = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&collision_store);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store.save_png("same", png, collision_now)
        }));
    }
    let mut collision_paths = BTreeSet::new();
    for handle in handles {
        let saved = handle
            .join()
            .map_err(|_| io::Error::other("output writer panicked"))??;
        assert!(saved.path.exists());
        collision_paths.insert(saved.path);
    }
    assert_eq!(collision_paths.len(), 8);

    let first = store.save_png("../../A B", png, instant()?)?;
    assert_eq!(
        first.path.file_name().and_then(|name| name.to_str()),
        Some("______A_B_20260115T000000Z.png")
    );
    assert_eq!(fs::read(&first.path)?, png);
    #[cfg(unix)]
    {
        assert_eq!(fs::metadata(&first.path)?.mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&root)?.mode() & 0o777, 0o700);
    }

    let stale = root.join("stale.png");
    fs::write(&stale, png)?;
    let file = fs::OpenOptions::new().write(true).open(&stale)?;
    file.set_times(fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))?;
    assert_eq!(store.cleanup(instant()?)?, 1);
    assert!(!stale.exists());

    store.save_png("second", png, instant()?)?;
    store.save_png("third", png, instant()?)?;
    let retained = fs::read_dir(&root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("png"))
        .count();
    assert_eq!(retained, 2);
    Ok(())
}

#[test]
fn invalid_output_policy_and_non_png_are_structured() -> Result<(), Box<dyn Error>> {
    assert!(OutputPolicy::new(Duration::ZERO, 1).is_err());
    assert!(OutputPolicy::new(Duration::days(1), 0).is_err());
    let temporary = tempfile::tempdir()?;
    let store = OutputStore::new(temporary.path().join("images"), OutputPolicy::standard())?;
    let error = store
        .save_png("b50", b"not png", instant()?)
        .err()
        .ok_or("expected invalid PNG error")?;
    assert!(matches!(error, B50ImageError::InvalidOutputPolicy { .. }));
    Ok(())
}

#[test]
fn service_maps_template_asset_errors_without_legacy_fallback() -> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let font = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf");
    let service = B50ImageService::new(
        StyleStore::new(temporary.path().join("style.json"), B50ImageStyle::Yuzu)?,
        OutputStore::new(temporary.path().join("images"), OutputPolicy::standard())?,
        LegacyRenderer::new(LegacyAssets::new(&font, &font))?,
        ResourceDirectories::new(
            temporary.path().join("static"),
            temporary.path().join("cache"),
        )?,
    );
    let view = B50View::new(
        "B50",
        PlayerHeader::new("Player", None, None)?,
        RatingBreakdown {
            b35: 0,
            b15: 0,
            total: 0,
        },
        Vec::new(),
        Vec::new(),
    )?;
    for (style, code) in [
        (B50ImageStyle::Yuzu, "YUZU_ASSETS_REQUIRED"),
        (B50ImageStyle::Maibot, "MAIBOT_ASSETS_REQUIRED"),
    ] {
        let error = service
            .render(
                &view,
                RenderOptions {
                    style: Some(style),
                    filename_stem: "b50".to_owned(),
                    ..RenderOptions::default()
                },
                instant()?,
            )
            .err()
            .ok_or("expected missing template assets")?;
        assert_eq!(error.code(), code);
        assert!(
            error
                .missing_assets()
                .is_some_and(|missing| !missing.is_empty())
        );
    }
    Ok(())
}
