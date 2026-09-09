use std::{error::Error, fs, path::Path};

use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, PlayAchievement, QqId,
    ScoreSource, SongIdNamespace, SourceSongId, UtageScore, ValidationError,
};
use maimai_storage::{PlayerProfile, PlayerRecord, StateStore};
use serde_json::json;
use sqlx::{Connection, Executor, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn chart_383() -> Result<ChartKey, ValidationError> {
    ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::OfficialCn, 383),
        ChartGeneration::Standard,
        Difficulty::Master,
    )
}

fn player_record(
    qq: &QqId,
    title: &str,
    achievements: &str,
) -> Result<PlayerRecord, Box<dyn Error + Send + Sync>> {
    Ok(PlayerRecord {
        qq: qq.clone(),
        chart: chart_383()?,
        title: title.to_owned(),
        level: Some("13+".to_owned()),
        level_label: Some("Master".to_owned()),
        ds: Some(ChartConstant::from_decimal_str("13.7")?),
        achievements: Some(AchievementRate::from_decimal_str(achievements)?.into()),
        dx_score: Some(2_458),
        fc: Some(maimai_core::FullComboStatus::FullComboPlus),
        fs: Some(maimai_core::FullSyncStatus::Sync),
        rate: Some("sssp".to_owned()),
        ra: Some(299),
        version: Some("maimai でらっくす".to_owned()),
        is_new: false,
        score_source: ScoreSource::OfficialCn,
        source_detail: Some("sdgb_raw_dump".to_owned()),
        raw: Some(json!({"musicId": 383, "achievement": achievements})),
        payload: json!({"title": title, "achievements": achievements}),
        updated_at: "2026-08-18T00:00:00Z".to_owned(),
    })
}

#[tokio::test]
async fn title_drift_for_id_383_updates_the_same_record() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("state.db");
    let store = StateStore::open(&database).await?;
    let qq = QqId::new("10001")?;

    store
        .upsert_record(&player_record(&qq, "Link(CoF)", "99.0000")?)
        .await?;
    store
        .upsert_record(&player_record(&qq, "Link (CoF)", "100.1234")?)
        .await?;

    let saved = store.record(&qq, &chart_383()?).await?;
    let records = store.records_for_player(&qq).await?;
    assert_eq!(
        saved.as_ref().map(|record| record.title.as_str()),
        Some("Link (CoF)")
    );
    assert_eq!(
        saved.and_then(|record| record.achievements),
        Some(AchievementRate::from_decimal_str("100.1234")?.into())
    );
    assert_eq!(records.len(), 1);
    store.close().await;

    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database)).await?;
    let row = sqlx::query(
        "SELECT typeof(ds) AS ds_type, ds, typeof(achievements) AS achievement_type, achievements FROM player_records_v3",
    )
    .fetch_one(&mut connection)
    .await?;
    assert_eq!(row.try_get::<String, _>("ds_type")?, "text");
    assert_eq!(row.try_get::<String, _>("ds")?, "13.7");
    assert_eq!(row.try_get::<String, _>("achievement_type")?, "text");
    assert_eq!(row.try_get::<String, _>("achievements")?, "100.1234");
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn utage_score_round_trips_exact_kind_units_and_decimal() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("utage.db");
    let store = StateStore::open(&database).await?;
    let qq = QqId::new("10011")?;
    let chart = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::OfficialCn, 111_597),
        ChartGeneration::UtageOnePlayer,
        Difficulty::Utage,
    )?;
    let mut record = player_record(&qq, "[息]ノンブレス・オブリージュ", "100")?;
    record.chart = chart.clone();
    record.level = Some("13+?".to_owned());
    record.level_label = Some("Utage".to_owned());
    record.ds = None;
    record.achievements = Some(PlayAchievement::from(UtageScore::from_ten_thousandths(
        1_535_756,
    )));
    record.ra = None;
    let serialized = serde_json::to_value(&record)?;
    assert_eq!(
        serde_json::from_value::<PlayerRecord>(serialized.clone())?,
        record
    );
    assert_eq!(
        serialized["achievements"],
        json!({"kind": "utage", "units": 1_535_756})
    );

    store.upsert_record(&record).await?;
    let saved = store.record(&qq, &chart).await?.ok_or("utage missing")?;
    assert_eq!(saved.achievements, record.achievements);
    store.close().await;

    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database)).await?;
    let row = sqlx::query(
        "SELECT achievements, achievement_kind, achievement_units FROM player_records_v3",
    )
    .fetch_one(&mut connection)
    .await?;
    assert_eq!(row.try_get::<String, _>("achievements")?, "153.5756");
    assert_eq!(row.try_get::<String, _>("achievement_kind")?, "utage");
    assert_eq!(row.try_get::<i64, _>("achievement_units")?, 1_535_756);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn display_titles_round_trip_without_trimming_in_both_write_paths() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let qq = QqId::new("10001")?;
    for title in ["　", "", "   ", " Padded title \t\n"] {
        let record = player_record(&qq, title, "98.3999")?;
        let profile = PlayerProfile {
            qq: qq.clone(),
            nickname: Some("　".to_owned()),
            player_rating: None,
            player_old_rating: None,
            player_new_rating: None,
            score_source: Some(record.score_source),
            source_detail: None,
            raw: None,
            updated_at: record.updated_at.clone(),
        };
        store
            .replace_player_score_snapshot(&profile, std::slice::from_ref(&record))
            .await?;
        let snapshot = store
            .full_score_snapshot(&qq, record.score_source, time::OffsetDateTime::UNIX_EPOCH)
            .await?
            .ok_or("snapshot missing")?;
        assert_eq!(snapshot.records()[0].title, title);
        assert_eq!(snapshot.profile().nickname, profile.nickname);
        store.upsert_record(&record).await?;
        let saved = store
            .record(&qq, &record.chart)
            .await?
            .ok_or("record missing")?;
        assert_eq!(saved.title, title);
        assert_eq!(saved.payload["title"], title);
        assert_eq!(store.records_for_player(&qq).await?.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn stored_achievement_kind_and_units_must_match_chart_and_domain() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("dirty-achievement.db");
    let store = StateStore::open(&database).await?;
    let qq = QqId::new("10012")?;
    store
        .upsert_record(&player_record(&qq, "Link", "100")?)
        .await?;
    store.close().await;

    let options = SqliteConnectOptions::new().filename(&database);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query("UPDATE player_records_v3 SET achievement_units = 1535756")
        .execute(&mut connection)
        .await?;
    connection.close().await?;

    let store = StateStore::open(&database).await?;
    assert!(matches!(
        store.record(&qq, &chart_383()?).await,
        Err(maimai_storage::StorageError::InvalidStoredValue {
            field: "player_records_v3.achievement_units",
            ..
        })
    ));
    store.close().await;

    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query(
        "UPDATE player_records_v3 SET achievement_kind = 'utage', achievement_units = 1000000",
    )
    .execute(&mut connection)
    .await?;
    connection.close().await?;
    let store = StateStore::open(&database).await?;
    assert!(matches!(
        store.record(&qq, &chart_383()?).await,
        Err(maimai_storage::StorageError::InvalidStoredValue {
            field: "player_records_v3.achievement_kind",
            ..
        })
    ));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn old_schema_backfill_only_classifies_values_valid_for_the_chart_kind() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("old-achievement-schema.db");
    let options = SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    connection
        .execute(
            r#"
            CREATE TABLE player_records_v3 (
                qq TEXT NOT NULL,
                source_namespace TEXT NOT NULL,
                source_value TEXT NOT NULL,
                generation TEXT NOT NULL,
                difficulty TEXT NOT NULL,
                title TEXT NOT NULL,
                level TEXT,
                level_label TEXT,
                ds TEXT,
                achievements TEXT,
                achievement_units INTEGER,
                dx_score INTEGER,
                fc TEXT,
                fs TEXT,
                rate TEXT,
                ra INTEGER,
                version TEXT,
                is_new INTEGER NOT NULL DEFAULT 0,
                score_source TEXT NOT NULL,
                source_detail TEXT,
                raw_json TEXT,
                payload_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (qq, source_namespace, source_value, generation, difficulty)
            )
            "#,
        )
        .await?;
    for (source_value, generation, difficulty, title) in [
        ("numeric:383", "standard", "master", "dirty regular"),
        ("numeric:111597", "utage_one_player", "utage", "valid utage"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO player_records_v3 (
                qq, source_namespace, source_value, generation, difficulty, title,
                achievements, achievement_units, score_source, payload_json, updated_at
            ) VALUES ('10013', 'official_cn', ?, ?, ?, ?, '153.5756', 1535756,
                      'official_cn', '{}', '2026-08-18T00:00:00Z')
            "#,
        )
        .bind(source_value)
        .bind(generation)
        .bind(difficulty)
        .bind(title)
        .execute(&mut connection)
        .await?;
    }
    connection.close().await?;

    let store = StateStore::open(&database).await?;
    let qq = QqId::new("10013")?;
    assert!(matches!(
        store.record(&qq, &chart_383()?).await,
        Err(maimai_storage::StorageError::InvalidStoredValue {
            field: "player_records_v3.achievement",
            ..
        })
    ));
    let utage_chart = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::OfficialCn, 111_597),
        ChartGeneration::UtageOnePlayer,
        Difficulty::Utage,
    )?;
    let utage = store
        .record(&qq, &utage_chart)
        .await?
        .ok_or("backfilled utage missing")?;
    assert_eq!(
        utage
            .achievements
            .and_then(PlayAchievement::utage)
            .map(UtageScore::ten_thousandths),
        Some(1_535_756)
    );
    store.close().await;

    let mut connection = SqliteConnection::connect_with(&options).await?;
    let rows = sqlx::query(
        "SELECT difficulty, achievement_kind FROM player_records_v3 ORDER BY difficulty",
    )
    .fetch_all(&mut connection)
    .await?;
    assert_eq!(rows[0].try_get::<String, _>("difficulty")?, "master");
    assert_eq!(
        rows[0].try_get::<Option<String>, _>("achievement_kind")?,
        None
    );
    assert_eq!(rows[1].try_get::<String, _>("achievement_kind")?, "utage");
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn stored_score_markers_accept_legacy_aliases_and_reject_unknown_values() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("markers.db");
    let store = StateStore::open(&database).await?;
    let qq = QqId::new("10009")?;
    store
        .upsert_record(&player_record(&qq, "Link", "100")?)
        .await?;
    store.close().await;

    let options = SqliteConnectOptions::new().filename(&database);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query("UPDATE player_records_v3 SET fc = ' FC+ ', fs = ' FDX+ '")
        .execute(&mut connection)
        .await?;
    connection.close().await?;

    let store = StateStore::open(&database).await?;
    let aliased = store
        .record(&qq, &chart_383()?)
        .await?
        .ok_or("record missing")?;
    assert_eq!(
        aliased.fc,
        Some(maimai_core::FullComboStatus::FullComboPlus)
    );
    assert_eq!(
        aliased.fs,
        Some(maimai_core::FullSyncStatus::FullSyncDeluxePlus)
    );
    store.close().await;

    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query("UPDATE player_records_v3 SET fc = 'clear'")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let store = StateStore::open(&database).await?;
    let error = store.record(&qq, &chart_383()?).await.err();
    assert!(matches!(
        error,
        Some(maimai_storage::StorageError::InvalidStoredValue {
            field: "player_records_v3.fc",
            ..
        })
    ));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn records_sort_achievements_by_exact_scaled_value() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let qq = QqId::new("10005")?;
    let mut lower = player_record(&qq, "Below 100", "99.9000")?;
    lower.chart = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::OfficialCn, 384),
        lower.chart.generation(),
        lower.chart.difficulty(),
    )?;
    lower.ra = None;
    let mut higher = player_record(&qq, "At 100", "100.0000")?;
    higher.chart = ChartKey::new(
        SourceSongId::numeric(SongIdNamespace::OfficialCn, 385),
        higher.chart.generation(),
        higher.chart.difficulty(),
    )?;
    higher.ra = None;

    store.upsert_record(&lower).await?;
    store.upsert_record(&higher).await?;

    let records = store.records_for_player(&qq).await?;
    assert_eq!(
        records
            .iter()
            .map(|record| record.title.as_str())
            .collect::<Vec<_>>(),
        ["At 100", "Below 100"]
    );
    store.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_upserts_keep_one_stable_record() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let qq = QqId::new("10002")?;
    let mut tasks = Vec::new();

    for index in 0_u32..32 {
        let worker_store = store.clone();
        let worker_qq = qq.clone();
        tasks.push(tokio::spawn(async move {
            let title = format!("Title revision {index}");
            let achievements = format!("99.{index:04}");
            let record = player_record(&worker_qq, &title, &achievements)?;
            worker_store.upsert_record(&record).await?;
            Ok::<(), Box<dyn Error + Send + Sync>>(())
        }));
    }
    for task in tasks {
        task.await??;
    }

    let records = store.records_for_player(&qq).await?;
    assert_eq!(records.len(), 1);
    assert!(records[0].title.starts_with("Title revision "));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn profile_and_typed_source_preference_round_trip() -> TestResult {
    let temp = TempDir::new()?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let qq = QqId::new("0010003")?;
    let profile = PlayerProfile {
        qq: qq.clone(),
        nickname: Some("Tester".to_owned()),
        player_rating: Some(15_000),
        player_old_rating: Some(10_000),
        player_new_rating: Some(5_000),
        score_source: Some(ScoreSource::Lxns),
        source_detail: None,
        raw: Some(json!({"name": "Tester"})),
        updated_at: "2026-08-18T00:00:00Z".to_owned(),
    };

    store.upsert_profile(&profile).await?;
    store
        .set_score_source_preference(&qq, ScoreSource::DivingFish)
        .await?;

    let stored_profile = store.profile(&qq).await?;
    let preference = store.score_source_preference(&qq).await?;
    assert_eq!(
        stored_profile.as_ref().and_then(|value| value.score_source),
        Some(ScoreSource::Lxns)
    );
    assert_eq!(
        stored_profile
            .as_ref()
            .and_then(|value| value.source_detail.as_deref()),
        Some("lxns")
    );
    assert_eq!(
        stored_profile.and_then(|value| value.nickname),
        profile.nickname
    );
    assert_eq!(preference, Some(ScoreSource::DivingFish));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn legacy_rows_are_preserved_and_only_stable_ids_are_imported_once() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("legacy.db");
    seed_legacy_database(&database).await?;

    let store = StateStore::open(&database).await?;
    assert_eq!(store.legacy_import_report().imported, 1);
    assert_eq!(store.legacy_import_report().skipped, 3);
    assert!(!store.legacy_import_report().already_applied);

    let qq = QqId::new("10004")?;
    let imported = store.record(&qq, &chart_383()?).await?;
    assert_eq!(
        imported.as_ref().map(|record| record.title.as_str()),
        Some("Legacy renamed final")
    );
    assert_eq!(
        imported.as_ref().and_then(|record| record.achievements),
        Some(AchievementRate::from_decimal_str("100.5")?.into())
    );
    assert_eq!(
        imported.as_ref().and_then(|record| record.ds),
        Some(ChartConstant::from_decimal_str("13.7")?)
    );
    let profile = store.profile(&qq).await?;
    assert_eq!(
        profile
            .as_ref()
            .and_then(|profile| profile.source_detail.as_deref()),
        Some("legacy_tool")
    );
    assert_eq!(profile.and_then(|profile| profile.score_source), None);
    store.close().await;

    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database)).await?;
    let legacy_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM local_records")
        .fetch_one(&mut connection)
        .await?;
    assert_eq!(legacy_count, 4);
    for table in [
        "local_profiles",
        "local_records",
        "local_b50_snapshots",
        "local_import_runs",
        "lxns_player_profiles",
        "player_records_v3",
        "score_source_preferences",
    ] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&mut connection)
        .await?;
        assert_eq!(exists, 1, "missing compatibility table: {table}");
    }
    connection.close().await?;

    let reopened = StateStore::open(&database).await?;
    assert!(reopened.legacy_import_report().already_applied);
    assert_eq!(reopened.legacy_import_report().imported, 1);
    assert_eq!(reopened.legacy_import_report().skipped, 3);
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn fresh_state_store_excludes_main_only_tables() -> TestResult {
    let temp = TempDir::new()?;
    let database = temp.path().join("state.db");
    let store = StateStore::open(&database).await?;
    store.close().await;

    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database)).await?;
    for table in [
        "user_bindings",
        "diving_fish_import_tokens",
        "official_cn_bindings",
        "lxns_friend_codes",
        "official_cn_b50_snapshots",
        "official_cn_b50_entries",
    ] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&mut connection)
        .await?;
        assert_eq!(exists, 0, "StateStore created main-only table: {table}");
    }
    connection.close().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn new_storage_paths_are_private_without_chmodding_shared_parent() -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new()?;
    let shared_parent = temp.path();
    let original_parent_mode = fs::metadata(shared_parent)?.permissions().mode() & 0o777;
    let private_dir = shared_parent.join("maimai-state");
    let database = private_dir.join("state.db");

    let store = StateStore::open(&database).await?;
    let private_mode = fs::metadata(&private_dir)?.permissions().mode() & 0o777;
    let database_mode = fs::metadata(&database)?.permissions().mode() & 0o777;
    let final_parent_mode = fs::metadata(shared_parent)?.permissions().mode() & 0o777;

    assert_eq!(private_mode, 0o700);
    assert_eq!(database_mode, 0o600);
    assert_eq!(final_parent_mode, original_parent_mode);
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = database.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = Path::new(&sidecar);
        if sidecar.exists() {
            let sidecar_mode = fs::metadata(sidecar)?.permissions().mode() & 0o777;
            assert_eq!(sidecar_mode, 0o600);
        }
    }
    store.close().await;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn existing_symlink_and_non_file_database_paths_are_rejected() -> TestResult {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new()?;
    let target = temp.path().join("target.db");
    fs::write(&target, b"do not touch")?;
    let link = temp.path().join("link.db");
    symlink(&target, &link)?;

    let symlink_result = StateStore::open(&link).await;
    assert!(symlink_result.is_err());
    assert_eq!(fs::read(&target)?, b"do not touch");

    let directory = temp.path().join("directory.db");
    fs::create_dir(&directory)?;
    let directory_result = StateStore::open(&directory).await;
    assert!(directory_result.is_err());
    Ok(())
}

async fn seed_legacy_database(path: &Path) -> TestResult {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    connection
        .execute(
            r#"
            CREATE TABLE local_profiles (
                qq TEXT PRIMARY KEY,
                nickname TEXT,
                player_rating INTEGER,
                player_old_rating INTEGER,
                player_new_rating INTEGER,
                source TEXT,
                raw_json TEXT,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .await?;
    connection
        .execute(
            r#"
            CREATE TABLE local_records (
                qq TEXT NOT NULL,
                title TEXT NOT NULL,
                type TEXT NOT NULL,
                level_index INTEGER NOT NULL,
                song_id INTEGER,
                level TEXT,
                level_label TEXT,
                ds REAL,
                achievements REAL,
                dx_score INTEGER,
                fc TEXT,
                fs TEXT,
                rate TEXT,
                ra INTEGER,
                version TEXT,
                is_new INTEGER NOT NULL DEFAULT 0,
                source TEXT,
                raw_json TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (qq, title, type, level_index)
            )
            "#,
        )
        .await?;
    sqlx::query(
        r#"
        INSERT INTO local_profiles (qq, nickname, source, raw_json, updated_at)
        VALUES ('10004', 'Legacy Player', 'legacy_tool', '{"kept":true}', '2026-08-17T00:00:00Z')
        "#,
    )
    .execute(&mut connection)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO local_records (
            qq, title, type, level_index, song_id, ds, achievements, source, raw_json, updated_at
        ) VALUES
            ('10004', 'Legacy title', 'SD', 3, 383, 13.7, 100.1, 'sdgb_raw_dump',
             '{"musicId":383}', '2026-08-17T00:00:00Z'),
            ('10004', 'Legacy renamed', 'SD', 3, 383, 13.7, 100.4, 'sdgb_raw_dump',
             '{"musicId":383}', '2026-08-18T00:00:00Z'),
            ('10004', 'Legacy renamed final', 'SD', 3, 383, 13.7, 100.5, 'sdgb_raw_dump',
             '{"musicId":383}', '2026-08-18T00:00:00Z'),
            ('10004', 'Ambiguous title', 'DX', 3, 999, 14.0, 99.9, 'sdgb_raw_dump',
             '{"song_id":999}', '2026-08-17T00:00:00Z')
        "#,
    )
    .execute(&mut connection)
    .await?;
    connection.close().await?;
    Ok(())
}
