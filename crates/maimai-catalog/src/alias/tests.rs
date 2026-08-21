use std::{error::Error, fs, path::Path, sync::Arc};

use maimai_core::SongIdValue;
use tempfile::TempDir;

use super::{
    AddAliasOutcome, AddAliasRequest, AddAliasResult, AliasKind, AliasListRequest, AliasListResult,
    AliasText, CanonicalName, DeleteAliasRequest, DeleteAliasResult, SongAliasTarget,
};
use crate::{CatalogFiles, CatalogStore, CatalogStoreError};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_three_kinds_are_idempotent_delete_and_hot_publish() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = CatalogStore::load(fixture.files.clone()).await?;
    let song_request = || -> Result<AddAliasRequest, super::AliasError> {
        Ok(AddAliasRequest::song(
            SongAliasTarget::Id(SongIdValue::Numeric(1)),
            AliasText::new("測試別名")?,
        ))
    };
    let first = store.add_alias(song_request()?).await?;
    assert_eq!(first.outcome(), AddAliasOutcome::Added);
    let second = store.add_alias(song_request()?).await?;
    assert_eq!(second.outcome(), AddAliasOutcome::AlreadyExists);
    assert_eq!(store.snapshot().search_text("测试别名", 5).len(), 1);

    for (kind, canonical) in [(AliasKind::Artist, "Alice"), (AliasKind::Charter, "Carol")] {
        let request = || {
            Ok::<_, Box<dyn Error>>(AddAliasRequest::name(
                kind,
                CanonicalName::new(canonical, kind)?,
                AliasText::new("測試別名")?,
            )?)
        };
        assert_eq!(
            store.add_alias(request()?).await?.outcome(),
            AddAliasOutcome::Added
        );
        assert_eq!(
            store.add_alias(request()?).await?.outcome(),
            AddAliasOutcome::AlreadyExists
        );
        let listed = store
            .list_aliases(AliasListRequest::new(
                kind,
                Some(AliasText::new("测试")?),
                20,
            )?)
            .await?;
        assert!(matches!(listed, AliasListResult::Names { ref entries, .. } if entries.len() == 1));
        let deleted = store
            .delete_alias(DeleteAliasRequest::name(
                kind,
                CanonicalName::new(canonical, kind)?,
                AliasText::new("测试别名")?,
            )?)
            .await?;
        assert!(matches!(deleted, DeleteAliasResult::Name(_)));
    }

    let deleted = store
        .delete_alias(DeleteAliasRequest::song(
            SongAliasTarget::Id(SongIdValue::Numeric(1)),
            AliasText::new("测试别名")?,
        ))
        .await?;
    assert!(matches!(deleted, DeleteAliasResult::Song(_)));
    assert!(store.snapshot().search_text("测试别名", 5).is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_adds_do_not_lose_updates() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = Arc::new(CatalogStore::load(fixture.files.clone()).await?);
    let add = |store: Arc<CatalogStore>, alias: &'static str| async move {
        store
            .add_alias(AddAliasRequest::song(
                SongAliasTarget::Id(SongIdValue::Numeric(1)),
                AliasText::new(alias)?,
            ))
            .await
    };
    let (first, second) = tokio::join!(
        add(Arc::clone(&store), "alpha-one"),
        add(Arc::clone(&store), "alpha-two")
    );
    assert!(matches!(first?, AddAliasResult::Song { .. }));
    assert!(matches!(second?, AddAliasResult::Song { .. }));
    let document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fixture.files.custom_aliases)?)?;
    assert_eq!(document["1"].as_array().map(Vec::len), Some(2));
    assert_eq!(store.snapshot().search_text("alpha-one", 5).len(), 1);
    assert_eq!(store.snapshot().search_text("alpha-two", 5).len(), 1);
    Ok(())
}

#[tokio::test]
async fn ambiguous_exact_title_is_a_typed_error() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fs::write(
        &fixture.files.lxns_song_list,
        song_document().replace(
            "\"songs\":[",
            "\"songs\":[{\"id\":2,\"title\":\"Twin\",\"artist\":\"A\",\"genre\":\"maimai\",\"bpm\":120,\"version\":10000,\"difficulties\":{\"standard\":[{\"difficulty\":3,\"level\":\"12\",\"level_value\":12.0,\"note_designer\":\"C\",\"notes\":{\"tap\":1}}]}},{\"id\":3,\"title\":\"Twin\",\"artist\":\"B\",\"genre\":\"maimai\",\"bpm\":120,\"version\":10000,\"difficulties\":{\"standard\":[{\"difficulty\":3,\"level\":\"12\",\"level_value\":12.0,\"note_designer\":\"D\",\"notes\":{\"tap\":1}}]}},",
        ),
    )?;
    let store = CatalogStore::load(fixture.files).await?;
    let result = store
        .add_alias(AddAliasRequest::song(
            SongAliasTarget::Title(super::SongTitle::new("Twin")?),
            AliasText::new("twins")?,
        ))
        .await;
    assert!(matches!(
        result,
        Err(CatalogStoreError::Alias(
            super::AliasError::AmbiguousSong { .. }
        ))
    ));
    Ok(())
}

#[tokio::test]
async fn jp_only_text_source_id_alias_hot_publishes() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fs::write(
        &fixture.files.lxns_song_list,
        r#"{"songs":[],"genres":[],"versions":[]}"#,
    )?;
    fs::write(&fixture.files.diving_fish_song_list, "[]")?;
    fs::write(
        fixture
            .files
            .dxdata
            .as_deref()
            .ok_or_else(|| std::io::Error::other("dxdata path missing"))?,
        r#"{"songs":[{"songId":"jp-special-id","title":"JP Only","artist":"A","category":"Game","bpm":120,"sheets":[{"type":"dx","difficulty":"master","level":"13","internalLevelValue":13.0,"noteDesigner":"C","internalId":12001}]}],"versions":[]}"#,
    )?;
    let store = CatalogStore::load(fixture.files).await?;
    store
        .add_alias(AddAliasRequest::song(
            SongAliasTarget::Id(SongIdValue::text("jp-special-id")?),
            AliasText::new("jp-only-alias")?,
        ))
        .await?;
    assert_eq!(store.snapshot().search_text("jp-only-alias", 5).len(), 1);
    Ok(())
}

#[tokio::test]
async fn written_alias_with_failed_reload_keeps_old_snapshot_and_is_recoverable()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = CatalogStore::load(fixture.files.clone()).await?;
    let before = store.snapshot();
    fs::write(&fixture.files.lxns_song_list, "broken")?;
    let result = store
        .add_alias(AddAliasRequest::song(
            SongAliasTarget::Id(SongIdValue::Numeric(1)),
            AliasText::new("recoverable")?,
        ))
        .await;
    let Err(error) = result else {
        return Err(std::io::Error::other("reload unexpectedly succeeded").into());
    };
    assert!(matches!(
        error,
        CatalogStoreError::AliasWrittenReloadFailed { .. }
    ));
    assert!(Arc::ptr_eq(&before, &store.snapshot()));
    fs::write(&fixture.files.lxns_song_list, song_document())?;
    let retry = store
        .add_alias(AddAliasRequest::song(
            SongAliasTarget::Id(SongIdValue::Numeric(1)),
            AliasText::new("recoverable")?,
        ))
        .await?;
    assert_eq!(retry.outcome(), AddAliasOutcome::AlreadyExists);
    assert_eq!(store.snapshot().search_text("recoverable", 5).len(), 1);
    Ok(())
}

#[tokio::test]
async fn invalid_alias_document_does_not_replace_snapshot() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = CatalogStore::load(fixture.files.clone()).await?;
    let before = store.snapshot();
    fs::write(&fixture.files.custom_aliases, "[]")?;
    let result = store
        .add_alias(AddAliasRequest::song(
            SongAliasTarget::Id(SongIdValue::Numeric(1)),
            AliasText::new("blocked-invalid")?,
        ))
        .await;
    assert!(matches!(
        result,
        Err(CatalogStoreError::Alias(
            super::AliasError::InvalidDocument { .. }
        ))
    ));
    assert!(Arc::ptr_eq(&before, &store.snapshot()));
    Ok(())
}

#[test]
fn atomic_replace_overwrites_twice_and_preserves_permissions() -> Result<(), Box<dyn Error>> {
    use super::file::{AliasDocument, write_atomic};

    let root = TempDir::new()?;
    let path = fs::canonicalize(root.path())?.join("aliases.json");
    fs::write(&path, "{}\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
    }
    write_atomic(
        &path,
        &AliasDocument::from([("1".to_owned(), vec!["one".to_owned()])]),
    )?;
    write_atomic(
        &path,
        &AliasDocument::from([("1".to_owned(), vec!["two".to_owned()])]),
    )?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    assert_eq!(value["1"][0], "two");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o640);
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symbolic_link_alias_target_is_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new()?;
    let store = CatalogStore::load(fixture.files.clone()).await?;
    let target = fixture.root.path().join("outside.json");
    fs::write(&target, "{}\n")?;
    fs::remove_file(&fixture.files.custom_aliases)?;
    symlink(&target, &fixture.files.custom_aliases)?;
    let result = store
        .add_alias(AddAliasRequest::song(
            SongAliasTarget::Id(SongIdValue::Numeric(1)),
            AliasText::new("blocked")?,
        ))
        .await;
    let Err(error) = result else {
        return Err(std::io::Error::other("symlink write unexpectedly succeeded").into());
    };
    assert!(matches!(
        error,
        CatalogStoreError::Alias(super::AliasError::SymbolicLink { .. })
    ));
    assert_eq!(fs::read_to_string(target)?, "{}\n");
    Ok(())
}

#[cfg(unix)]
#[test]
fn symbolic_link_parent_component_is_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    use super::file::{AliasDocument, write_atomic};

    let root = TempDir::new()?;
    let root_path = fs::canonicalize(root.path())?;
    let real = root_path.join("real");
    let linked = root_path.join("linked");
    fs::create_dir(&real)?;
    symlink(&real, &linked)?;
    let result = write_atomic(&linked.join("aliases.json"), &AliasDocument::new());
    let Err(error) = result else {
        return Err(std::io::Error::other("symlink parent unexpectedly accepted").into());
    };
    assert!(matches!(error, super::AliasError::SymbolicLink { .. }));
    Ok(())
}

struct Fixture {
    root: TempDir,
    files: CatalogFiles,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = TempDir::new()?;
        let files = CatalogFiles::from_data_dir(fs::canonicalize(root.path())?);
        write(&files.lxns_song_list, &song_document())?;
        write(&files.diving_fish_song_list, "[]")?;
        write(&files.lxns_alias_list, r#"{"aliases":[]}"#)?;
        write(&files.yuzu_alias_list, r#"{"content":[]}"#)?;
        write(&files.custom_aliases, "{}")?;
        write(&files.pinyin_aliases, r#"{"aliases":[]}"#)?;
        write(
            &files.simplified_to_traditional,
            r#"{"测":"測","试":"試","别":"別"}"#,
        )?;
        write_optional(files.artist_aliases.as_deref(), "{}")?;
        write_optional(files.charter_aliases.as_deref(), "{}")?;
        write_optional(
            files.traditional_to_simplified.as_deref(),
            r#"{"測":"测","試":"试","別":"别"}"#,
        )?;
        Ok(Self { root, files })
    }
}

fn write(path: &Path, value: &str) -> Result<(), std::io::Error> {
    fs::write(path, value)
}

fn write_optional(path: Option<&Path>, value: &str) -> Result<(), Box<dyn Error>> {
    write(
        path.ok_or_else(|| std::io::Error::other("path not configured"))?,
        value,
    )?;
    Ok(())
}

fn song_document() -> String {
    r#"{
      "songs":[{"id":1,"title":"Alpha","artist":"Alice","genre":"maimai","bpm":120,"version":10000,
      "difficulties":{"standard":[{"difficulty":3,"level":"12","level_value":12.0,
      "note_designer":"Carol","notes":{"tap":1}}]}}],
      "genres":[{"title":"maimai","genre":"maimai"}],
      "versions":[{"title":"maimai","version":10000}]
    }"#
    .to_owned()
}
