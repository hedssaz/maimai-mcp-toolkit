use maimai_core::{GroupId, PlayerUsername, QqId};
use time::OffsetDateTime;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentitySnapshotMember {
    pub qq: QqId,
    pub nickname: Option<String>,
    pub card: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityGroupSnapshot {
    pub group_id: GroupId,
    pub group_name: Option<String>,
    pub member_count: Option<u64>,
    pub members: Vec<IdentitySnapshotMember>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentitySnapshot {
    pub fetched_at: OffsetDateTime,
    pub friends: Vec<IdentitySnapshotMember>,
    pub groups: Vec<IdentityGroupSnapshot>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WaterfishIdentityProfile {
    pub nickname: Option<String>,
    pub username: Option<PlayerUsername>,
    pub rating: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityGroupMembership {
    pub group_id: GroupId,
    pub group_name: Option<String>,
    pub group_nickname: String,
    pub card: Option<String>,
    pub nickname: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityRecord {
    pub qq: QqId,
    pub qq_nickname: Option<String>,
    pub friend_nickname: Option<String>,
    pub preferred_group: Option<IdentityGroupMembership>,
    pub groups: Vec<IdentityGroupMembership>,
    pub waterfish_nickname: Option<String>,
    pub waterfish_username: Option<PlayerUsername>,
    pub waterfish_rating: Option<u32>,
    pub is_friend: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IdentityStats {
    pub friend_count: u64,
    pub group_count: u64,
    pub group_member_rows: u64,
    pub unique_users: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityMetadata {
    pub fetched_at: Option<OffsetDateTime>,
    pub updated_at: OffsetDateTime,
    pub generation: u64,
    pub stats: IdentityStats,
}
