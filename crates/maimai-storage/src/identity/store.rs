use std::{collections::BTreeMap, str::FromStr};

use maimai_core::{GroupId, PlayerUsername, QqId};
use sqlx::{Row, sqlite::SqliteRow};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    IdentityGroupMembership, IdentityMetadata, IdentityRecord, IdentitySnapshot,
    WaterfishIdentityProfile,
    metadata::{identity_stats, metadata_from_row, next_generation, stored_integer},
};
use crate::{StateStore, StorageError};

impl StateStore {
    pub async fn replace_identity_snapshot(
        &self,
        snapshot: &IdentitySnapshot,
    ) -> Result<IdentityMetadata, StorageError> {
        let timestamp = snapshot.fetched_at.format(&Rfc3339)?;
        let mut transaction = self.pool.begin().await?;
        let generation = next_generation(&mut transaction).await?;

        sqlx::query("DELETE FROM identity_members")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM identity_groups")
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE identity_users SET is_friend = 0, qq_nickname = NULL, friend_nickname = NULL",
        )
        .execute(&mut *transaction)
        .await?;

        for friend in &snapshot.friends {
            let nickname = clean_text(friend.nickname.as_deref());
            sqlx::query(
                r#"
                INSERT INTO identity_users (qq, is_friend, qq_nickname, friend_nickname)
                VALUES (?, 1, ?, ?)
                ON CONFLICT (qq) DO UPDATE SET
                    is_friend = 1,
                    qq_nickname = COALESCE(identity_users.qq_nickname, excluded.qq_nickname),
                    friend_nickname = excluded.friend_nickname
                "#,
            )
            .bind(friend.qq.as_str())
            .bind(&nickname)
            .bind(&nickname)
            .execute(&mut *transaction)
            .await?;
        }

        for group in &snapshot.groups {
            let group_name = clean_text(group.group_name.as_deref());
            let member_count = group
                .member_count
                .map(|count| stored_integer(count, "member_count"))
                .transpose()?;
            sqlx::query(
                r#"
                INSERT INTO identity_groups (group_id, group_name, member_count, updated_at)
                VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(group.group_id.as_str())
            .bind(&group_name)
            .bind(member_count)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await?;

            for member in &group.members {
                let nickname = clean_text(member.nickname.as_deref());
                let card = clean_text(member.card.as_deref());
                let group_nickname = card
                    .as_deref()
                    .or(nickname.as_deref())
                    .unwrap_or_else(|| member.qq.as_str());
                sqlx::query(
                    r#"
                    INSERT INTO identity_users (qq, qq_nickname)
                    VALUES (?, ?)
                    ON CONFLICT (qq) DO UPDATE SET
                        qq_nickname = COALESCE(excluded.qq_nickname, identity_users.qq_nickname)
                    "#,
                )
                .bind(member.qq.as_str())
                .bind(&nickname)
                .execute(&mut *transaction)
                .await?;
                sqlx::query(
                    r#"
                    INSERT INTO identity_members (
                        group_id, qq, group_nickname, card, nickname, updated_at
                    ) VALUES (?, ?, ?, ?, ?, ?)
                    "#,
                )
                .bind(group.group_id.as_str())
                .bind(member.qq.as_str())
                .bind(group_nickname)
                .bind(&card)
                .bind(&nickname)
                .bind(&timestamp)
                .execute(&mut *transaction)
                .await?;
            }
        }

        sqlx::query(
            r#"
            DELETE FROM identity_users
            WHERE is_friend = 0
              AND waterfish_nickname IS NULL
              AND waterfish_username IS NULL
              AND waterfish_rating IS NULL
              AND NOT EXISTS (
                  SELECT 1 FROM identity_members WHERE identity_members.qq = identity_users.qq
              )
            "#,
        )
        .execute(&mut *transaction)
        .await?;

        let stats = identity_stats(&mut transaction).await?;
        sqlx::query(
            r#"
            INSERT INTO identity_metadata (
                singleton, fetched_at, updated_at, generation,
                friend_count, group_count, group_member_rows, unique_users
            ) VALUES (1, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (singleton) DO UPDATE SET
                fetched_at = excluded.fetched_at,
                updated_at = excluded.updated_at,
                generation = excluded.generation,
                friend_count = excluded.friend_count,
                group_count = excluded.group_count,
                group_member_rows = excluded.group_member_rows,
                unique_users = excluded.unique_users
            "#,
        )
        .bind(&timestamp)
        .bind(&timestamp)
        .bind(stored_integer(generation, "generation")?)
        .bind(stored_integer(stats.friend_count, "friend_count")?)
        .bind(stored_integer(stats.group_count, "group_count")?)
        .bind(stored_integer(
            stats.group_member_rows,
            "group_member_rows",
        )?)
        .bind(stored_integer(stats.unique_users, "unique_users")?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        Ok(IdentityMetadata {
            fetched_at: Some(snapshot.fetched_at),
            updated_at: snapshot.fetched_at,
            generation,
            stats,
        })
    }

    pub async fn upsert_waterfish_identity(
        &self,
        qq: &QqId,
        profile: &WaterfishIdentityProfile,
        updated_at: OffsetDateTime,
    ) -> Result<(), StorageError> {
        let nickname = clean_text(profile.nickname.as_deref());
        let username = profile.username.as_ref().map(PlayerUsername::as_str);
        let timestamp = updated_at.format(&Rfc3339)?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO identity_users (
                qq, waterfish_nickname, waterfish_username,
                waterfish_rating, waterfish_updated_at
            ) VALUES (?, ?, ?, ?, ?)
            ON CONFLICT (qq) DO UPDATE SET
                waterfish_nickname = COALESCE(
                    excluded.waterfish_nickname, identity_users.waterfish_nickname
                ),
                waterfish_username = COALESCE(
                    excluded.waterfish_username, identity_users.waterfish_username
                ),
                waterfish_rating = COALESCE(
                    excluded.waterfish_rating, identity_users.waterfish_rating
                ),
                waterfish_updated_at = excluded.waterfish_updated_at
            "#,
        )
        .bind(qq.as_str())
        .bind(nickname)
        .bind(username)
        .bind(profile.rating.map(i64::from))
        .bind(timestamp)
        .execute(&mut *transaction)
        .await?;
        let stats = identity_stats(&mut transaction).await?;
        sqlx::query(
            r#"
            INSERT INTO identity_metadata (
                singleton, fetched_at, updated_at, generation,
                friend_count, group_count, group_member_rows, unique_users
            ) VALUES (1, NULL, ?, 0, ?, ?, ?, ?)
            ON CONFLICT (singleton) DO UPDATE SET
                updated_at = excluded.updated_at,
                friend_count = excluded.friend_count,
                group_count = excluded.group_count,
                group_member_rows = excluded.group_member_rows,
                unique_users = excluded.unique_users
            "#,
        )
        .bind(updated_at.format(&Rfc3339)?)
        .bind(stored_integer(stats.friend_count, "friend_count")?)
        .bind(stored_integer(stats.group_count, "group_count")?)
        .bind(stored_integer(
            stats.group_member_rows,
            "group_member_rows",
        )?)
        .bind(stored_integer(stats.unique_users, "unique_users")?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn identity_metadata(&self) -> Result<Option<IdentityMetadata>, StorageError> {
        let row = sqlx::query("SELECT * FROM identity_metadata WHERE singleton = 1")
            .fetch_optional(&self.pool)
            .await?;
        row.map(metadata_from_row).transpose()
    }

    pub async fn identity(
        &self,
        qq: &QqId,
        preferred_group: Option<&GroupId>,
    ) -> Result<Option<IdentityRecord>, StorageError> {
        let row = sqlx::query("SELECT * FROM identity_users WHERE qq = ?")
            .bind(qq.as_str())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut identity = identity_from_row(row)?;
        let memberships = sqlx::query(
            r#"
            SELECT m.group_id, g.group_name, m.group_nickname, m.card, m.nickname
            FROM identity_members m
            JOIN identity_groups g ON g.group_id = m.group_id
            WHERE m.qq = ?
            ORDER BY COALESCE(g.group_name, ''), m.group_id
            "#,
        )
        .bind(qq.as_str())
        .fetch_all(&self.pool)
        .await?;
        for row in memberships {
            append_membership(&mut identity, membership_from_row(row)?, preferred_group);
        }
        Ok(Some(identity))
    }

    pub async fn identities(
        &self,
        preferred_group: Option<&GroupId>,
    ) -> Result<Vec<IdentityRecord>, StorageError> {
        let rows = sqlx::query("SELECT * FROM identity_users ORDER BY qq")
            .fetch_all(&self.pool)
            .await?;
        let mut identities = BTreeMap::new();
        for row in rows {
            let identity = identity_from_row(row)?;
            identities.insert(identity.qq.as_str().to_owned(), identity);
        }

        let memberships = sqlx::query(
            r#"
            SELECT m.qq, m.group_id, g.group_name, m.group_nickname, m.card, m.nickname
            FROM identity_members m
            JOIN identity_groups g ON g.group_id = m.group_id
            ORDER BY COALESCE(g.group_name, ''), m.group_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        for row in memberships {
            let qq: String = row.try_get("qq")?;
            let membership = membership_from_row(row)?;
            if let Some(identity) = identities.get_mut(&qq) {
                append_membership(identity, membership, preferred_group);
            }
        }
        Ok(identities.into_values().collect())
    }
}

fn identity_from_row(row: SqliteRow) -> Result<IdentityRecord, StorageError> {
    let qq_text: String = row.try_get("qq")?;
    let qq = QqId::from_str(&qq_text).map_err(|_| invalid("qq", qq_text))?;
    let username: Option<String> = row.try_get("waterfish_username")?;
    let waterfish_username = username
        .map(|value| PlayerUsername::new(&value).map_err(|_| invalid("waterfish_username", value)))
        .transpose()?;
    Ok(IdentityRecord {
        qq,
        qq_nickname: row.try_get("qq_nickname")?,
        friend_nickname: row.try_get("friend_nickname")?,
        preferred_group: None,
        groups: Vec::new(),
        waterfish_nickname: row.try_get("waterfish_nickname")?,
        waterfish_username,
        waterfish_rating: optional_u32(&row, "waterfish_rating")?,
        is_friend: row.try_get::<i64, _>("is_friend")? != 0,
    })
}

fn membership_from_row(row: SqliteRow) -> Result<IdentityGroupMembership, StorageError> {
    let group_id_text: String = row.try_get("group_id")?;
    let group_id =
        GroupId::new(&group_id_text).map_err(|_| invalid("group_id", group_id_text.clone()))?;
    Ok(IdentityGroupMembership {
        group_id,
        group_name: row.try_get("group_name")?,
        group_nickname: row.try_get("group_nickname")?,
        card: row.try_get("card")?,
        nickname: row.try_get("nickname")?,
    })
}

fn append_membership(
    identity: &mut IdentityRecord,
    membership: IdentityGroupMembership,
    preferred_group: Option<&GroupId>,
) {
    if preferred_group.is_some_and(|preferred| preferred == &membership.group_id) {
        identity.preferred_group = Some(membership.clone());
    }
    identity.groups.push(membership);
}

fn optional_u32(
    row: &sqlx::sqlite::SqliteRow,
    field: &'static str,
) -> Result<Option<u32>, StorageError> {
    let value: Option<i64> = row.try_get(field)?;
    value
        .map(|value| u32::try_from(value).map_err(|_| invalid(field, value.to_string())))
        .transpose()
}

fn clean_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn invalid(field: &'static str, value: impl Into<String>) -> StorageError {
    StorageError::InvalidStoredValue {
        field,
        value: value.into(),
    }
}
