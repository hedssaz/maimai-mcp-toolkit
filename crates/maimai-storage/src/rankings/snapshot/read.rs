use std::collections::BTreeMap;

use maimai_core::{ChartKey, QqId, RatingBreakdown, SourceSongId};
use sqlx::{Row, sqlite::SqliteRow};

use super::{invalid, snapshot_from_row, unsigned_u32};
use crate::{
    B50Section, CachedB50Chart, CachedB50Entry, CachedChart, CachedExactRatio, CachedFitIndex,
    CachedFitIndexLabel, CachedFitIndexSection, CachedPlayer, RankingCache, RankingMember,
    RankingNamespace, RankingSnapshotData, StateStore, StorageError,
    store::codec::{
        achievement_rate_from_db, chart_constant_from_db, difficulty_from_db, generation_from_db,
        namespace_from_db, qq_from_db, source_value_from_db,
    },
};

impl StateStore {
    pub async fn ranking_cache(
        &self,
        namespace: RankingNamespace,
        group_id: &maimai_core::GroupId,
    ) -> Result<Option<RankingCache>, StorageError> {
        let row =
            sqlx::query("SELECT * FROM ranking_snapshots WHERE namespace = ? AND group_id = ?")
                .bind(namespace.as_str())
                .bind(group_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let snapshot = snapshot_from_row(&row)?;
        let members = read_members(self, &snapshot).await?;
        let data = match namespace {
            RankingNamespace::B50 => {
                RankingSnapshotData::B50(read_b50_entries(self, &snapshot, members).await?)
            }
            RankingNamespace::SongScore => RankingSnapshotData::SongScores {
                members,
                records: read_song_records(self, &snapshot).await?,
            },
        };
        Ok(Some(RankingCache { snapshot, data }))
    }
}

async fn read_members(
    store: &StateStore,
    snapshot: &crate::RankingSnapshot,
) -> Result<Vec<RankingMember>, StorageError> {
    let rows = sqlx::query(
        r#"SELECT ordinal, qq, nickname, card, display_name, waterfish_nickname,
                  waterfish_username
           FROM ranking_members
           WHERE namespace = ? AND group_id = ? AND generation = ?
           ORDER BY ordinal"#,
    )
    .bind(snapshot.namespace.as_str())
    .bind(snapshot.group_id.as_str())
    .bind(i64::try_from(snapshot.generation).map_err(|_| {
        invalid(
            "ranking_members.generation",
            snapshot.generation.to_string(),
        )
    })?)
    .fetch_all(&store.pool)
    .await?;
    rows.into_iter().map(member_from_row).collect()
}

async fn read_b50_entries(
    store: &StateStore,
    snapshot: &crate::RankingSnapshot,
    members: Vec<RankingMember>,
) -> Result<Vec<CachedB50Entry>, StorageError> {
    let generation = i64::try_from(snapshot.generation).map_err(|_| {
        invalid(
            "ranking_b50_entries.generation",
            snapshot.generation.to_string(),
        )
    })?;
    let chart_rows = sqlx::query(
        r#"SELECT * FROM ranking_b50_charts
           WHERE group_id = ? AND generation = ? ORDER BY qq, section, ordinal"#,
    )
    .bind(snapshot.group_id.as_str())
    .bind(generation)
    .fetch_all(&store.pool)
    .await?;
    let mut charts = BTreeMap::<String, Vec<CachedB50Chart>>::new();
    for row in chart_rows {
        let qq: String = row.try_get("qq")?;
        let section_text: String = row.try_get("section")?;
        let section = B50Section::from_stored(&section_text)
            .ok_or_else(|| invalid("ranking_b50_charts.section", section_text))?;
        charts.entry(qq).or_default().push(CachedB50Chart {
            section,
            ordinal: unsigned_u32(&row, "ordinal")?,
            chart: chart_from_row(&row)?,
        });
    }

    let rows = sqlx::query(
        "SELECT * FROM ranking_b50_entries WHERE group_id = ? AND generation = ? ORDER BY qq",
    )
    .bind(snapshot.group_id.as_str())
    .bind(generation)
    .fetch_all(&store.pool)
    .await?;
    let fit_rows =
        sqlx::query("SELECT * FROM ranking_b50_fit_sections WHERE group_id = ? AND generation = ?")
            .bind(snapshot.group_id.as_str())
            .bind(generation)
            .fetch_all(&store.pool)
            .await?;
    let mut fit_sections = BTreeMap::new();
    for fit_row in fit_rows {
        let qq: String = fit_row.try_get("qq")?;
        let section: String = fit_row.try_get("section")?;
        fit_sections.insert((qq, section), fit_section_from_row(&fit_row)?);
    }
    let mut members = members
        .into_iter()
        .map(|member| (member.qq.as_str().to_owned(), member))
        .collect::<BTreeMap<_, _>>();
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let qq: String = row.try_get("qq")?;
        let member = members
            .remove(&qq)
            .ok_or_else(|| invalid("ranking_b50_entries.qq", qq.clone()))?;
        entries.push(CachedB50Entry {
            member,
            player: CachedPlayer {
                nickname: row.try_get("player_nickname")?,
                username: row.try_get("player_username")?,
                rating: optional_u32(&row, "player_rating")?,
                actual_rating: optional_u32(&row, "player_actual_rating")?,
                additional_rating: optional_u32(&row, "player_additional_rating")?,
                plate: row.try_get("player_plate")?,
            },
            rating_breakdown: RatingBreakdown {
                b35: required_u32(&row, "b35_rating")?,
                b15: required_u32(&row, "b15_rating")?,
                total: required_u32(&row, "total_rating")?,
            },
            fit_index: CachedFitIndex {
                label: row
                    .try_get::<Option<String>, _>("fit_label")?
                    .map(|value| {
                        CachedFitIndexLabel::from_stored(&value)
                            .ok_or_else(|| invalid("ranking_b50_entries.fit_label", value))
                    })
                    .transpose()?,
                b50: fit_sections
                    .remove(&(qq.clone(), "b50".to_owned()))
                    .ok_or_else(|| invalid("ranking_b50_fit_sections.b50", qq.clone()))?,
                b35: fit_sections
                    .remove(&(qq.clone(), "b35".to_owned()))
                    .ok_or_else(|| invalid("ranking_b50_fit_sections.b35", qq.clone()))?,
                b15: fit_sections
                    .remove(&(qq.clone(), "b15".to_owned()))
                    .ok_or_else(|| invalid("ranking_b50_fit_sections.b15", qq.clone()))?,
            },
            charts: charts.remove(&qq).unwrap_or_default(),
        });
    }
    entries.sort_by_key(|entry| entry.member.ordinal);
    Ok(entries)
}

async fn read_song_records(
    store: &StateStore,
    snapshot: &crate::RankingSnapshot,
) -> Result<Vec<(QqId, CachedChart)>, StorageError> {
    let generation = i64::try_from(snapshot.generation).map_err(|_| {
        invalid(
            "ranking_song_entries.generation",
            snapshot.generation.to_string(),
        )
    })?;
    let rows = sqlx::query(
        r#"SELECT * FROM ranking_song_entries
           WHERE group_id = ? AND generation = ?
           ORDER BY qq, source_namespace, source_value, chart_generation, difficulty"#,
    )
    .bind(snapshot.group_id.as_str())
    .bind(generation)
    .fetch_all(&store.pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let qq = qq_from_db(row.try_get("qq")?)?;
            Ok((qq, chart_from_row(&row)?))
        })
        .collect()
}

fn member_from_row(row: SqliteRow) -> Result<RankingMember, StorageError> {
    Ok(RankingMember {
        ordinal: unsigned_u32(&row, "ordinal")?,
        qq: qq_from_db(row.try_get("qq")?)?,
        nickname: row.try_get("nickname")?,
        card: row.try_get("card")?,
        display_name: row.try_get("display_name")?,
        waterfish_nickname: row.try_get("waterfish_nickname")?,
        waterfish_username: row.try_get("waterfish_username")?,
    })
}

fn chart_from_row(row: &SqliteRow) -> Result<CachedChart, StorageError> {
    let namespace: String = row.try_get("source_namespace")?;
    let value: String = row.try_get("source_value")?;
    let generation: String = row.try_get("chart_generation")?;
    let difficulty: String = row.try_get("difficulty")?;
    let chart_generation = generation_from_db(&generation)?;
    let chart_difficulty = difficulty_from_db(&difficulty)?;
    let key = ChartKey::new(
        SourceSongId::new(
            namespace_from_db(&namespace)?,
            source_value_from_db(&value)?,
        ),
        chart_generation,
        chart_difficulty,
    )
    .map_err(|_| StorageError::InvalidStoredValue {
        field: "ranking_chart.chart",
        value: format!("{namespace}/{value}/{generation}/{difficulty}"),
    })?;
    Ok(CachedChart {
        key,
        title: row.try_get("title")?,
        level: row.try_get("level")?,
        constant: optional_text(row, "constant")?
            .map(|value| chart_constant_from_db(&value))
            .transpose()?,
        achievements: optional_text(row, "achievements")?
            .map(|value| achievement_rate_from_db(&value))
            .transpose()?,
        dx_score: optional_u32(row, "dx_score")?,
        rating: optional_u32(row, "rating")?,
        original_rating: optional_u32(row, "original_rating")?,
        grade: row.try_get("grade")?,
        full_combo: optional_marker(row, "full_combo")?,
        full_sync: optional_marker(row, "full_sync")?,
        version: row.try_get("version")?,
        is_current: row.try_get::<i64, _>("is_current")? != 0,
        fit_constant: optional_text(row, "fit_constant")?
            .map(|value| chart_constant_from_db(&value))
            .transpose()?,
    })
}

fn optional_text(row: &SqliteRow, field: &'static str) -> Result<Option<String>, StorageError> {
    row.try_get(field).map_err(StorageError::from)
}

fn optional_marker<T>(row: &SqliteRow, field: &'static str) -> Result<Option<T>, StorageError>
where
    T: std::str::FromStr,
{
    optional_text(row, field)?
        .map(|value| {
            value
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue { field, value })
        })
        .transpose()
}

fn optional_u32(row: &SqliteRow, field: &'static str) -> Result<Option<u32>, StorageError> {
    let value: Option<i64> = row.try_get(field)?;
    value
        .map(|value| u32::try_from(value).map_err(|_| invalid(field, value.to_string())))
        .transpose()
}

fn required_u32(row: &SqliteRow, field: &'static str) -> Result<u32, StorageError> {
    let value: i64 = row.try_get(field)?;
    u32::try_from(value).map_err(|_| invalid(field, value.to_string()))
}

fn optional_i128(row: &SqliteRow, field: &'static str) -> Result<Option<i128>, StorageError> {
    row.try_get::<Option<String>, _>(field)?
        .map(|value| value.parse::<i128>().map_err(|_| invalid(field, value)))
        .transpose()
}

fn optional_u128(row: &SqliteRow, field: &'static str) -> Result<Option<u128>, StorageError> {
    row.try_get::<Option<String>, _>(field)?
        .map(|value| value.parse::<u128>().map_err(|_| invalid(field, value)))
        .transpose()
}

fn fit_section_from_row(row: &SqliteRow) -> Result<CachedFitIndexSection, StorageError> {
    let virtual_numerator = optional_i128(row, "virtual_ratio_numerator")?;
    let virtual_denominator = optional_u128(row, "virtual_ratio_denominator")?;
    let weighted_numerator = optional_i128(row, "weighted_delta_numerator")?;
    let weighted_denominator = optional_u128(row, "weighted_delta_denominator")?;
    Ok(CachedFitIndexSection {
        virtual_rating: row.try_get("virtual_rating")?,
        virtual_ratio_percent: ratio(virtual_numerator, virtual_denominator, "virtual_ratio")?,
        weighted_average_delta: ratio(weighted_numerator, weighted_denominator, "weighted_delta")?,
        counted: required_u32(row, "counted")?,
        missing: required_u32(row, "missing")?,
        total_rating: row
            .try_get::<Option<i64>, _>("total_rating")?
            .map(|value| {
                u64::try_from(value).map_err(|_| invalid("total_rating", value.to_string()))
            })
            .transpose()?,
    })
}

fn ratio(
    numerator: Option<i128>,
    denominator: Option<u128>,
    field: &'static str,
) -> Result<Option<CachedExactRatio>, StorageError> {
    match (numerator, denominator) {
        (None, None) => Ok(None),
        (Some(numerator), Some(denominator)) if denominator > 0 => Ok(Some(CachedExactRatio {
            numerator,
            denominator,
        })),
        _ => Err(invalid(field, "incomplete ratio")),
    }
}
