use maimai_core::{GroupId, QqId};
use maimai_providers::NapCatClient;
use maimai_storage::RankingMember;
use time::OffsetDateTime;

use crate::identity::{GroupMemberPolicy, IdentityQuery, MaxResults, ResetHour};

use super::{RankingError, RankingService, RefreshOptions};

impl RankingService {
    pub(crate) async fn group_members(
        &self,
        group_id: &GroupId,
        client: &NapCatClient,
        options: RefreshOptions,
        now: OffsetDateTime,
    ) -> Result<Vec<RankingMember>, RankingError> {
        let policy = GroupMemberPolicy {
            no_cache: options.no_cache,
            max_members: options.max_members.map(|value| value.get()),
        };
        let reset = ResetHour::new(14)?;
        self.identity
            .group_members_with_client(client, group_id, policy, now, reset)
            .await?
            .into_iter()
            .enumerate()
            .map(|(index, member)| {
                Ok(RankingMember {
                    ordinal: u32::try_from(index).map_err(|_| RankingError::Task)?,
                    qq: member.qq,
                    nickname: member.nickname,
                    card: member.card,
                    display_name: member.group_nickname,
                    waterfish_nickname: member.waterfish_nickname,
                    waterfish_username: member.waterfish_username,
                })
            })
            .collect()
    }

    pub async fn resolve_member(
        &self,
        qq: Option<QqId>,
        target: Option<&str>,
        group_id: Option<GroupId>,
    ) -> Result<(QqId, GroupId), RankingError> {
        let qq = match (qq, target) {
            (Some(qq), _) => qq,
            (None, Some(target)) => {
                if let Ok(qq) = QqId::new(target) {
                    qq
                } else {
                    let query = IdentityQuery::new(target)?;
                    let resolution = self
                        .identity
                        .resolve_identity(&query, group_id.as_ref(), MaxResults::default_value())
                        .await?;
                    if resolution.ambiguous {
                        return Err(RankingError::Ambiguous);
                    }
                    resolution
                        .matches
                        .first()
                        .map(|value| value.identity.qq.clone())
                        .ok_or(RankingError::NotFound)?
                }
            }
            (None, None) => {
                return Err(RankingError::InvalidInput {
                    field: "qq or target",
                });
            }
        };
        let group_id = match group_id {
            Some(group_id) => group_id,
            None => {
                let identity = self
                    .identity
                    .get_identity(&qq, None)
                    .await?
                    .ok_or(RankingError::NotFound)?;
                let mut groups = identity
                    .groups
                    .into_iter()
                    .map(|group| group.group_id)
                    .collect::<Vec<_>>();
                groups.sort();
                groups.dedup();
                if groups.len() != 1 {
                    return Err(if groups.is_empty() {
                        RankingError::NotFound
                    } else {
                        RankingError::Ambiguous
                    });
                }
                groups.pop().ok_or(RankingError::NotFound)?
            }
        };
        Ok((qq, group_id))
    }
}
