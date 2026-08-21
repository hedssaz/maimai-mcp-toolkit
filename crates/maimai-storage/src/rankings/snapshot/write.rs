use sqlx::{Sqlite, Transaction};
use time::format_description::well_known::Rfc3339;

use super::{stored_u32, stored_u64};
use crate::{
    CachedB50Chart, CachedB50Entry, CachedChart, CachedFitIndexSection, RankingMember,
    RankingNamespace, RankingSnapshot, StateStore, StorageError,
    store::codec::{
        achievement_rate_db, chart_constant_db, difficulty_db, generation_db, namespace_db,
        source_value_db,
    },
};

impl StateStore {
    pub async fn replace_b50_ranking(
        &self,
        snapshot: &RankingSnapshot,
        entries: &[CachedB50Entry],
    ) -> Result<(), StorageError> {
        require_namespace(snapshot, RankingNamespace::B50)?;
        let mut transaction = self.pool.begin().await?;
        replace_snapshot(&mut transaction, snapshot).await?;
        for entry in entries {
            insert_member(&mut transaction, snapshot, &entry.member).await?;
            insert_b50_entry(&mut transaction, snapshot, entry).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn complete_b50_ranking(
        &self,
        snapshot: &RankingSnapshot,
        entries: &[CachedB50Entry],
        finished_at: time::OffsetDateTime,
    ) -> Result<(), StorageError> {
        require_namespace(snapshot, RankingNamespace::B50)?;
        let mut transaction = self.pool.begin().await?;
        replace_snapshot(&mut transaction, snapshot).await?;
        for entry in entries {
            insert_member(&mut transaction, snapshot, &entry.member).await?;
            insert_b50_entry(&mut transaction, snapshot, entry).await?;
        }
        super::super::job::complete_job_in_transaction(&mut transaction, snapshot, finished_at)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn replace_song_ranking(
        &self,
        snapshot: &RankingSnapshot,
        members: &[RankingMember],
        records: &[(maimai_core::QqId, CachedChart)],
    ) -> Result<(), StorageError> {
        require_namespace(snapshot, RankingNamespace::SongScore)?;
        let mut transaction = self.pool.begin().await?;
        replace_snapshot(&mut transaction, snapshot).await?;
        for member in members {
            insert_member(&mut transaction, snapshot, member).await?;
        }
        for (qq, chart) in records {
            insert_song_chart(&mut transaction, snapshot, qq, chart).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn complete_song_ranking(
        &self,
        snapshot: &RankingSnapshot,
        members: &[RankingMember],
        records: &[(maimai_core::QqId, CachedChart)],
        finished_at: time::OffsetDateTime,
    ) -> Result<(), StorageError> {
        require_namespace(snapshot, RankingNamespace::SongScore)?;
        let mut transaction = self.pool.begin().await?;
        replace_snapshot(&mut transaction, snapshot).await?;
        for member in members {
            insert_member(&mut transaction, snapshot, member).await?;
        }
        for (qq, chart) in records {
            insert_song_chart(&mut transaction, snapshot, qq, chart).await?;
        }
        super::super::job::complete_job_in_transaction(&mut transaction, snapshot, finished_at)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn clear_ranking_cache(
        &self,
        namespace: RankingNamespace,
        group_id: &maimai_core::GroupId,
    ) -> Result<bool, StorageError> {
        let result =
            sqlx::query("DELETE FROM ranking_snapshots WHERE namespace = ? AND group_id = ?")
                .bind(namespace.as_str())
                .bind(group_id.as_str())
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() == 1)
    }
}

async fn replace_snapshot(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &RankingSnapshot,
) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM ranking_snapshots WHERE namespace = ? AND group_id = ?")
        .bind(snapshot.namespace.as_str())
        .bind(snapshot.group_id.as_str())
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        r#"INSERT INTO ranking_snapshots (
            namespace, group_id, generation, fetched_at, next_reset_at, member_count,
            success_count, failure_count, skipped_count, cache_hit_count, shared_fetch_count
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(snapshot.namespace.as_str())
    .bind(snapshot.group_id.as_str())
    .bind(stored_u64(
        snapshot.generation,
        "ranking_snapshots.generation",
    )?)
    .bind(snapshot.fetched_at.format(&Rfc3339)?)
    .bind(snapshot.next_reset_at.format(&Rfc3339)?)
    .bind(stored_u32(snapshot.member_count))
    .bind(stored_u32(snapshot.success_count))
    .bind(stored_u32(snapshot.failure_count))
    .bind(stored_u32(snapshot.skipped_count))
    .bind(stored_u32(snapshot.cache_hit_count))
    .bind(stored_u32(snapshot.shared_fetch_count))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_member(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &RankingSnapshot,
    member: &RankingMember,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO ranking_members (
            namespace, group_id, generation, ordinal, qq, nickname, card, display_name,
            waterfish_nickname, waterfish_username
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(snapshot.namespace.as_str())
    .bind(snapshot.group_id.as_str())
    .bind(stored_u64(
        snapshot.generation,
        "ranking_members.generation",
    )?)
    .bind(stored_u32(member.ordinal))
    .bind(member.qq.as_str())
    .bind(&member.nickname)
    .bind(&member.card)
    .bind(&member.display_name)
    .bind(&member.waterfish_nickname)
    .bind(&member.waterfish_username)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_b50_entry(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &RankingSnapshot,
    entry: &CachedB50Entry,
) -> Result<(), StorageError> {
    let player = &entry.player;
    sqlx::query(
        r#"INSERT INTO ranking_b50_entries (
            namespace, group_id, generation, qq, player_nickname, player_username,
            player_rating, player_actual_rating, player_additional_rating, player_plate,
            b35_rating, b15_rating, total_rating, fit_label
        ) VALUES ('b50', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(snapshot.group_id.as_str())
    .bind(stored_u64(
        snapshot.generation,
        "ranking_b50_entries.generation",
    )?)
    .bind(entry.member.qq.as_str())
    .bind(&player.nickname)
    .bind(&player.username)
    .bind(player.rating.map(i64::from))
    .bind(player.actual_rating.map(i64::from))
    .bind(player.additional_rating.map(i64::from))
    .bind(&player.plate)
    .bind(i64::from(entry.rating_breakdown.b35))
    .bind(i64::from(entry.rating_breakdown.b15))
    .bind(i64::from(entry.rating_breakdown.total))
    .bind(entry.fit_index.label.map(|label| label.as_str()))
    .execute(&mut **transaction)
    .await?;
    for (section, value) in [
        ("b50", entry.fit_index.b50),
        ("b35", entry.fit_index.b35),
        ("b15", entry.fit_index.b15),
    ] {
        insert_fit_section(transaction, snapshot, &entry.member.qq, section, value).await?;
    }
    for chart in &entry.charts {
        insert_b50_chart(transaction, snapshot, &entry.member.qq, chart).await?;
    }
    Ok(())
}

async fn insert_fit_section(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &RankingSnapshot,
    qq: &maimai_core::QqId,
    section: &str,
    value: CachedFitIndexSection,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO ranking_b50_fit_sections (
            group_id, generation, qq, section, virtual_rating,
            virtual_ratio_numerator, virtual_ratio_denominator,
            weighted_delta_numerator, weighted_delta_denominator,
            counted, missing, total_rating
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(snapshot.group_id.as_str())
    .bind(stored_u64(
        snapshot.generation,
        "ranking_b50_fit_sections.generation",
    )?)
    .bind(qq.as_str())
    .bind(section)
    .bind(value.virtual_rating)
    .bind(
        value
            .virtual_ratio_percent
            .map(|ratio| ratio.numerator.to_string()),
    )
    .bind(
        value
            .virtual_ratio_percent
            .map(|ratio| ratio.denominator.to_string()),
    )
    .bind(
        value
            .weighted_average_delta
            .map(|ratio| ratio.numerator.to_string()),
    )
    .bind(
        value
            .weighted_average_delta
            .map(|ratio| ratio.denominator.to_string()),
    )
    .bind(stored_u32(value.counted))
    .bind(stored_u32(value.missing))
    .bind(
        value
            .total_rating
            .map(|value| stored_u64(value, "ranking_b50_fit_sections.total_rating"))
            .transpose()?,
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_b50_chart(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &RankingSnapshot,
    qq: &maimai_core::QqId,
    cached: &CachedB50Chart,
) -> Result<(), StorageError> {
    let chart = &cached.chart;
    let key = &chart.key;
    sqlx::query(
        r#"INSERT INTO ranking_b50_charts (
            group_id, generation, qq, section, ordinal, source_namespace, source_value,
            chart_generation, difficulty, title, level, constant, achievements, dx_score,
            rating, original_rating, grade, full_combo, full_sync, version, is_current, fit_constant
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(snapshot.group_id.as_str())
    .bind(stored_u64(
        snapshot.generation,
        "ranking_b50_charts.generation",
    )?)
    .bind(qq.as_str())
    .bind(cached.section.as_str())
    .bind(stored_u32(cached.ordinal))
    .bind(namespace_db(key.song().namespace()))
    .bind(source_value_db(key.song().value()))
    .bind(generation_db(key.generation()))
    .bind(difficulty_db(key.difficulty()))
    .bind(&chart.title)
    .bind(&chart.level)
    .bind(chart.constant.map(chart_constant_db))
    .bind(chart.achievements.map(achievement_rate_db))
    .bind(chart.dx_score.map(i64::from))
    .bind(chart.rating.map(i64::from))
    .bind(chart.original_rating.map(i64::from))
    .bind(&chart.grade)
    .bind(chart.full_combo.map(maimai_core::FullComboStatus::as_str))
    .bind(chart.full_sync.map(maimai_core::FullSyncStatus::as_str))
    .bind(&chart.version)
    .bind(i64::from(chart.is_current))
    .bind(chart.fit_constant.map(chart_constant_db))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_song_chart(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &RankingSnapshot,
    qq: &maimai_core::QqId,
    chart: &CachedChart,
) -> Result<(), StorageError> {
    let key = &chart.key;
    sqlx::query(
        r#"INSERT INTO ranking_song_entries (
            namespace, group_id, generation, qq, source_namespace, source_value, chart_generation,
            difficulty, title, level, constant, achievements, dx_score, rating, original_rating,
            grade, full_combo, full_sync, version, is_current, fit_constant
        ) VALUES ('song_score', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(snapshot.group_id.as_str())
    .bind(stored_u64(
        snapshot.generation,
        "ranking_song_entries.generation",
    )?)
    .bind(qq.as_str())
    .bind(namespace_db(key.song().namespace()))
    .bind(source_value_db(key.song().value()))
    .bind(generation_db(key.generation()))
    .bind(difficulty_db(key.difficulty()))
    .bind(&chart.title)
    .bind(&chart.level)
    .bind(chart.constant.map(chart_constant_db))
    .bind(chart.achievements.map(achievement_rate_db))
    .bind(chart.dx_score.map(i64::from))
    .bind(chart.rating.map(i64::from))
    .bind(chart.original_rating.map(i64::from))
    .bind(&chart.grade)
    .bind(chart.full_combo.map(maimai_core::FullComboStatus::as_str))
    .bind(chart.full_sync.map(maimai_core::FullSyncStatus::as_str))
    .bind(&chart.version)
    .bind(i64::from(chart.is_current))
    .bind(chart.fit_constant.map(chart_constant_db))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn require_namespace(
    snapshot: &RankingSnapshot,
    expected: RankingNamespace,
) -> Result<(), StorageError> {
    if snapshot.namespace == expected {
        Ok(())
    } else {
        Err(StorageError::InvalidStoredValue {
            field: "ranking_snapshots.namespace",
            value: snapshot.namespace.as_str().to_owned(),
        })
    }
}
