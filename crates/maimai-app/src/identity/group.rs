use maimai_core::{GroupId, QqId};
use maimai_providers::NapCatClient;
use time::OffsetDateTime;

use super::{IdentityDirectory, IdentityError, IdentityService, ResetHour};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupMemberPolicy {
    pub no_cache: bool,
    pub max_members: Option<usize>,
}

impl Default for GroupMemberPolicy {
    fn default() -> Self {
        Self {
            no_cache: true,
            max_members: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityGroupMember {
    pub qq: QqId,
    pub nickname: Option<String>,
    pub card: Option<String>,
    pub group_nickname: String,
    pub waterfish_nickname: Option<String>,
    pub waterfish_username: Option<String>,
}

impl IdentityService {
    pub async fn group_members(
        &self,
        group_id: &GroupId,
        policy: GroupMemberPolicy,
        now: OffsetDateTime,
        reset_hour: ResetHour,
    ) -> Result<Vec<IdentityGroupMember>, IdentityError> {
        self.group_members_with_client(&self.napcat, group_id, policy, now, reset_hour)
            .await
    }

    pub async fn group_members_with_client(
        &self,
        client: &NapCatClient,
        group_id: &GroupId,
        policy: GroupMemberPolicy,
        now: OffsetDateTime,
        reset_hour: ResetHour,
    ) -> Result<Vec<IdentityGroupMember>, IdentityError> {
        let status = self.cache_status(now, reset_hour).await?;
        if status.fresh {
            let cached = self.directory.cached_group_members(group_id).await?;
            if !cached.is_empty() {
                return Ok(truncate(cached, policy.max_members));
            }
        }
        let remote = client
            .get_group_member_list(group_id.as_str(), policy.no_cache)
            .await?
            .into_data();
        let mut members = Vec::with_capacity(remote.len());
        for value in remote {
            let qq = QqId::new(value.user_id())
                .map_err(|_| IdentityError::InvalidRemoteIdentity { field: "user_id" })?;
            let identity = self.directory.get_identity(&qq, Some(group_id)).await?;
            members.push(IdentityGroupMember {
                qq,
                nickname: value.nickname().map(str::to_owned),
                card: value.card().map(str::to_owned),
                group_nickname: value
                    .card()
                    .or(value.nickname())
                    .unwrap_or(value.user_id())
                    .to_owned(),
                waterfish_nickname: identity
                    .as_ref()
                    .and_then(|identity| identity.waterfish_nickname.clone()),
                waterfish_username: identity
                    .as_ref()
                    .and_then(|identity| identity.waterfish_username.as_ref())
                    .map(|username| username.as_str().to_owned()),
            });
        }
        Ok(truncate(members, policy.max_members))
    }
}

impl IdentityDirectory {
    pub async fn cached_group_members(
        &self,
        group_id: &GroupId,
    ) -> Result<Vec<IdentityGroupMember>, IdentityError> {
        let mut members = Vec::new();
        for identity in self.store().identities(Some(group_id)).await? {
            let membership = identity
                .groups
                .iter()
                .find(|membership| &membership.group_id == group_id)
                .or(identity
                    .preferred_group
                    .as_ref()
                    .filter(|membership| &membership.group_id == group_id));
            let Some(membership) = membership else {
                continue;
            };
            members.push(IdentityGroupMember {
                qq: identity.qq,
                nickname: membership.nickname.clone().or(identity.qq_nickname),
                card: membership.card.clone(),
                group_nickname: membership.group_nickname.clone(),
                waterfish_nickname: identity.waterfish_nickname,
                waterfish_username: identity
                    .waterfish_username
                    .map(|username| username.as_str().to_owned()),
            });
        }
        Ok(members)
    }
}

fn truncate(
    mut members: Vec<IdentityGroupMember>,
    max_members: Option<usize>,
) -> Vec<IdentityGroupMember> {
    if let Some(max_members) = max_members {
        members.truncate(max_members);
    }
    members
}
