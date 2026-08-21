use sqlx::SqlitePool;

use crate::StorageError;

const TABLES: &[&str] = &[
    r#"CREATE TABLE IF NOT EXISTS ranking_snapshots (
        namespace TEXT NOT NULL CHECK (namespace IN ('b50', 'song_score')),
        group_id TEXT NOT NULL,
        generation INTEGER NOT NULL CHECK (generation > 0),
        fetched_at TEXT NOT NULL,
        next_reset_at TEXT NOT NULL,
        member_count INTEGER NOT NULL CHECK (member_count >= 0),
        success_count INTEGER NOT NULL CHECK (success_count >= 0),
        failure_count INTEGER NOT NULL CHECK (failure_count >= 0),
        skipped_count INTEGER NOT NULL CHECK (skipped_count >= 0),
        cache_hit_count INTEGER NOT NULL CHECK (cache_hit_count >= 0),
        shared_fetch_count INTEGER NOT NULL CHECK (shared_fetch_count >= 0),
        PRIMARY KEY (namespace, group_id),
        UNIQUE (namespace, group_id, generation)
    ) STRICT"#,
    r#"CREATE TABLE IF NOT EXISTS ranking_members (
        namespace TEXT NOT NULL,
        group_id TEXT NOT NULL,
        generation INTEGER NOT NULL,
        ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
        qq TEXT NOT NULL,
        nickname TEXT,
        card TEXT,
        display_name TEXT NOT NULL,
        waterfish_nickname TEXT,
        waterfish_username TEXT,
        PRIMARY KEY (namespace, group_id, generation, qq),
        UNIQUE (namespace, group_id, generation, ordinal),
        FOREIGN KEY (namespace, group_id, generation)
            REFERENCES ranking_snapshots(namespace, group_id, generation) ON DELETE CASCADE
    ) STRICT"#,
    r#"CREATE TABLE IF NOT EXISTS ranking_b50_entries (
        namespace TEXT NOT NULL DEFAULT 'b50' CHECK (namespace = 'b50'),
        group_id TEXT NOT NULL,
        generation INTEGER NOT NULL,
        qq TEXT NOT NULL,
        player_nickname TEXT,
        player_username TEXT,
        player_rating INTEGER,
        player_actual_rating INTEGER,
        player_additional_rating INTEGER,
        player_plate TEXT,
        b35_rating INTEGER NOT NULL,
        b15_rating INTEGER NOT NULL,
        total_rating INTEGER NOT NULL,
        fit_label TEXT,
        PRIMARY KEY (group_id, generation, qq),
        FOREIGN KEY (namespace, group_id, generation, qq)
            REFERENCES ranking_members(namespace, group_id, generation, qq) ON DELETE CASCADE
    ) STRICT"#,
    r#"CREATE TABLE IF NOT EXISTS ranking_b50_fit_sections (
        group_id TEXT NOT NULL,
        generation INTEGER NOT NULL,
        qq TEXT NOT NULL,
        section TEXT NOT NULL CHECK (section IN ('b50', 'b35', 'b15')),
        virtual_rating INTEGER,
        virtual_ratio_numerator TEXT,
        virtual_ratio_denominator TEXT,
        weighted_delta_numerator TEXT,
        weighted_delta_denominator TEXT,
        counted INTEGER NOT NULL CHECK (counted >= 0),
        missing INTEGER NOT NULL CHECK (missing >= 0),
        total_rating INTEGER,
        PRIMARY KEY (group_id, generation, qq, section),
        FOREIGN KEY (group_id, generation, qq)
            REFERENCES ranking_b50_entries(group_id, generation, qq) ON DELETE CASCADE
    ) STRICT"#,
    r#"CREATE TABLE IF NOT EXISTS ranking_b50_charts (
        group_id TEXT NOT NULL,
        generation INTEGER NOT NULL,
        qq TEXT NOT NULL,
        section TEXT NOT NULL CHECK (section IN ('b35', 'b15')),
        ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
        source_namespace TEXT NOT NULL,
        source_value TEXT NOT NULL,
        chart_generation TEXT NOT NULL,
        difficulty TEXT NOT NULL,
        title TEXT NOT NULL,
        level TEXT NOT NULL,
        constant TEXT,
        achievements TEXT,
        dx_score INTEGER,
        rating INTEGER,
        original_rating INTEGER,
        grade TEXT,
        full_combo TEXT,
        full_sync TEXT,
        version TEXT NOT NULL,
        is_current INTEGER NOT NULL CHECK (is_current IN (0, 1)),
        fit_constant TEXT,
        PRIMARY KEY (group_id, generation, qq, section, ordinal),
        UNIQUE (group_id, generation, qq, source_namespace, source_value, chart_generation, difficulty),
        FOREIGN KEY (group_id, generation, qq)
            REFERENCES ranking_b50_entries(group_id, generation, qq) ON DELETE CASCADE
    ) STRICT"#,
    r#"CREATE TABLE IF NOT EXISTS ranking_song_entries (
        namespace TEXT NOT NULL DEFAULT 'song_score' CHECK (namespace = 'song_score'),
        group_id TEXT NOT NULL,
        generation INTEGER NOT NULL,
        qq TEXT NOT NULL,
        source_namespace TEXT NOT NULL,
        source_value TEXT NOT NULL,
        chart_generation TEXT NOT NULL,
        difficulty TEXT NOT NULL,
        title TEXT NOT NULL,
        level TEXT NOT NULL,
        constant TEXT,
        achievements TEXT,
        dx_score INTEGER,
        rating INTEGER,
        original_rating INTEGER,
        grade TEXT,
        full_combo TEXT,
        full_sync TEXT,
        version TEXT NOT NULL,
        is_current INTEGER NOT NULL CHECK (is_current IN (0, 1)),
        fit_constant TEXT,
        PRIMARY KEY (group_id, generation, qq, source_namespace, source_value, chart_generation, difficulty),
        FOREIGN KEY (namespace, group_id, generation, qq)
            REFERENCES ranking_members(namespace, group_id, generation, qq) ON DELETE CASCADE
    ) STRICT"#,
    r#"CREATE TABLE IF NOT EXISTS ranking_jobs (
        namespace TEXT NOT NULL CHECK (namespace IN ('b50', 'song_score')),
        group_id TEXT NOT NULL,
        generation INTEGER NOT NULL CHECK (generation > 0),
        status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'interrupted')),
        started_at TEXT NOT NULL,
        finished_at TEXT,
        refresh_reason TEXT NOT NULL CHECK (refresh_reason IN ('miss', 'stale', 'force_refresh')),
        message TEXT NOT NULL,
        processed_count INTEGER NOT NULL DEFAULT 0,
        total_count INTEGER,
        cached_count INTEGER NOT NULL DEFAULT 0,
        progress_skipped_count INTEGER NOT NULL DEFAULT 0,
        transient_failure_count INTEGER NOT NULL DEFAULT 0,
        current_qq TEXT,
        member_count INTEGER,
        success_count INTEGER,
        skipped_count INTEGER,
        error_code TEXT,
        error_message TEXT,
        error_status INTEGER,
        error_body TEXT,
        PRIMARY KEY (namespace, group_id)
    ) STRICT"#,
];

pub(crate) async fn initialize_rankings_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in TABLES {
        sqlx::query(*statement).execute(pool).await?;
    }
    Ok(())
}
