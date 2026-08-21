use sqlx::SqlitePool;

use crate::StorageError;

const TABLES: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS local_profiles (
        qq TEXT PRIMARY KEY,
        nickname TEXT,
        player_rating INTEGER,
        player_old_rating INTEGER,
        player_new_rating INTEGER,
        source TEXT,
        raw_json TEXT,
        upper_profile_json TEXT,
        upper_profile_version TEXT,
        upper_profile_basic_updated_at TEXT,
        upper_profile_collection_updated_at TEXT,
        upper_render_image_path TEXT,
        upper_render_signature TEXT,
        upper_render_updated_at TEXT,
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS local_records (
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
        payload_json TEXT,
        source_record_json TEXT,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (qq, title, type, level_index)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_local_records_qq ON local_records (qq)",
    "CREATE INDEX IF NOT EXISTS idx_local_records_song ON local_records (song_id, type, level_index)",
    r#"
    CREATE TABLE IF NOT EXISTS local_b50_snapshots (
        qq TEXT PRIMARY KEY,
        source TEXT,
        computed_at TEXT NOT NULL,
        record_count INTEGER NOT NULL,
        sd_count INTEGER NOT NULL,
        dx_count INTEGER NOT NULL,
        sd_rating INTEGER NOT NULL,
        dx_rating INTEGER NOT NULL,
        total_rating INTEGER NOT NULL,
        result_json TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS local_import_runs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        qq TEXT NOT NULL,
        source TEXT,
        raw_json TEXT,
        payload_json TEXT,
        saved_count INTEGER NOT NULL,
        skipped_count INTEGER NOT NULL,
        created_at TEXT NOT NULL
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_local_import_runs_qq ON local_import_runs (qq, created_at)",
    r#"
    CREATE TABLE IF NOT EXISTS lxns_player_profiles (
        qq TEXT PRIMARY KEY,
        player_json TEXT NOT NULL,
        upper_profile_json TEXT NOT NULL,
        player_upload_time TEXT,
        fetched_at TEXT NOT NULL,
        upper_render_image_path TEXT,
        upper_render_signature TEXT,
        upper_render_updated_at TEXT,
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS player_records_v3 (
        qq TEXT NOT NULL,
        source_namespace TEXT NOT NULL CHECK (source_namespace IN (
            'diving_fish', 'lxns', 'official_cn', 'dx_rating', 'yuzu'
        )),
        source_value TEXT NOT NULL,
        generation TEXT NOT NULL CHECK (generation IN (
            'standard', 'deluxe', 'utage_one_player', 'utage_two_player'
        )),
        difficulty TEXT NOT NULL CHECK (difficulty IN (
            'basic', 'advanced', 'expert', 'master', 're_master', 'utage'
        )),
        title TEXT NOT NULL,
        level TEXT,
        level_label TEXT,
        ds TEXT,
        achievements TEXT,
        achievement_kind TEXT CHECK (achievement_kind IN ('ranked', 'utage')),
        achievement_units INTEGER,
        dx_score INTEGER,
        fc TEXT,
        fs TEXT,
        rate TEXT,
        ra INTEGER,
        version TEXT,
        is_new INTEGER NOT NULL DEFAULT 0 CHECK (is_new IN (0, 1)),
        score_source TEXT NOT NULL CHECK (score_source IN (
            'diving_fish', 'lxns', 'local', 'official_cn'
        )),
        source_detail TEXT,
        raw_json TEXT,
        payload_json TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (qq, source_namespace, source_value, generation, difficulty),
        CHECK (
            (achievement_kind IS NULL AND achievement_units IS NULL)
            OR (achievement_kind IS NOT NULL AND achievement_units BETWEEN 0 AND 4294967295)
        )
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_player_records_v3_qq ON player_records_v3 (qq)",
    r#"
    CREATE TABLE IF NOT EXISTS player_score_snapshots (
        qq TEXT PRIMARY KEY,
        score_source TEXT NOT NULL CHECK (score_source IN (
            'diving_fish', 'lxns', 'local', 'official_cn'
        )),
        fetched_at TEXT NOT NULL,
        record_count INTEGER NOT NULL CHECK (record_count >= 0)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS score_source_preferences (
        qq TEXT PRIMARY KEY,
        score_source TEXT NOT NULL CHECK (score_source IN (
            'diving_fish', 'lxns', 'local', 'official_cn'
        )),
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS maimai_storage_migrations (
        name TEXT PRIMARY KEY,
        applied_at TEXT NOT NULL,
        imported_count INTEGER NOT NULL,
        skipped_count INTEGER NOT NULL
    )
    "#,
];

pub(super) async fn initialize_base_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in TABLES {
        sqlx::query(*statement).execute(pool).await?;
    }
    Ok(())
}
