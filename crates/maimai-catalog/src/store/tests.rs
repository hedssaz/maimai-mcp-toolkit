use std::{error::Error, fs, path::Path, sync::Arc};

use tempfile::TempDir;

use super::{CatalogFiles, CatalogRevision, CatalogStore, ReloadOutcome};
use crate::{PlateName, PlateQuery, PlateServer};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_reads_observe_a_complete_snapshot() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = Arc::new(CatalogStore::load(fixture.files.clone()).await?);
    let mut readers = Vec::new();
    for _ in 0..32 {
        let store = Arc::clone(&store);
        readers.push(tokio::spawn(async move {
            let snapshot = store.snapshot();
            snapshot.songs().first().map(|song| song.title.clone())
        }));
    }
    for reader in readers {
        assert_eq!(reader.await?, Some("Version One".to_owned()));
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_reload_keeps_the_published_snapshot() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = CatalogStore::load(fixture.files.clone()).await?;
    let before = store.snapshot();
    fs::write(
        &fixture.files.lxns_song_list,
        "not-json-and-a-different-size",
    )?;

    assert!(store.reload().await.is_err());
    let after = store.snapshot();
    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(
        after.songs().first().map(|song| song.title.as_str()),
        Some("Version One")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_revision_triggers_one_reload() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = Arc::new(CatalogStore::load(fixture.files.clone()).await?);
    fs::write(
        &fixture.files.lxns_song_list,
        song_document("Version Two Extended"),
    )?;

    let (first, second) = tokio::join!(store.reload_if_changed(), store.reload_if_changed());
    let outcomes = [first?, second?];
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.changed()).count(),
        1
    );
    let changed = outcomes
        .iter()
        .find(|outcome| outcome.changed())
        .ok_or_else(|| std::io::Error::other("missing changed reload outcome"))?;
    assert_eq!(
        changed
            .snapshot()
            .songs()
            .first()
            .map(|song| song.title.as_str()),
        Some("Version Two Extended")
    );
    let published = store.published.load_full();
    assert!(Arc::ptr_eq(&published.snapshot, changed.snapshot()));
    assert_eq!(published.revision, CatalogRevision::read(&fixture.files)?);
    assert!(matches!(
        store.reload_if_changed().await?,
        ReloadOutcome::Unchanged(_)
    ));
    Ok(())
}

#[tokio::test]
async fn custom_plate_sidecar_participates_in_revision_and_atomic_snapshot()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = CatalogStore::load(fixture.files.clone()).await?;
    let path = fixture
        .files
        .custom_plates
        .as_ref()
        .ok_or_else(|| std::io::Error::other("custom plate path missing"))?;
    fs::write(
        path,
        r#"{"content":{"Fixture":{"songs":[{"title":"External","type":"DX","level":["1","2","3","12"],"ds":[1.0,2.0,3.0,12.0]}]}}}"#,
    )?;
    assert!(store.reload_if_changed().await?.changed());
    let members = store.snapshot().plate_members(&PlateQuery::new(
        PlateName::new("Fixture")?,
        PlateServer::Custom,
    ));
    assert_eq!(members.declared_song_count(), 1);
    assert_eq!(members.members()[0].title(), "External");
    Ok(())
}

struct Fixture {
    _root: TempDir,
    files: CatalogFiles,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = TempDir::new()?;
        let files = CatalogFiles::from_data_dir(root.path());
        write_fixture(&files)?;
        Ok(Self { _root: root, files })
    }
}

fn write_fixture(files: &CatalogFiles) -> Result<(), Box<dyn Error>> {
    write(&files.lxns_song_list, &song_document("Version One"))?;
    write(&files.diving_fish_song_list, "[]")?;
    write(&files.lxns_alias_list, r#"{"aliases":[]}"#)?;
    write(&files.yuzu_alias_list, r#"{"content":[]}"#)?;
    write(&files.custom_aliases, "{}")?;
    write(&files.pinyin_aliases, r#"{"aliases":[]}"#)?;
    write(&files.simplified_to_traditional, "{}")?;
    Ok(())
}

fn write(path: &Path, value: &str) -> Result<(), std::io::Error> {
    fs::write(path, value)
}

fn song_document(title: &str) -> String {
    format!(
        r#"{{
              "songs": [{{
                "id": 1,
                "title": "{title}",
                "artist": "Tester",
                "genre": "maimai",
                "bpm": 120,
                "version": 10000,
                "difficulties": {{
                  "standard": [{{
                    "difficulty": 3,
                    "level": "12",
                    "level_value": 12.0,
                    "note_designer": "Tester",
                    "notes": {{"tap": 1, "hold": 0, "slide": 0, "touch": 0, "break": 0}}
                  }}]
                }}
              }}],
              "genres": [{{"title": "maimai", "genre": "maimai"}}],
              "versions": [{{"title": "maimai", "version": 10000}}]
            }}"#
    )
}
