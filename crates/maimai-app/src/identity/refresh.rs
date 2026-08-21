use std::{collections::BTreeSet, sync::Arc};

use maimai_core::{GroupId, QqId};
use maimai_providers::NapCatClient;
use maimai_storage::{
    IdentityGroupSnapshot, IdentityJobProgress, IdentitySnapshot, IdentitySnapshotMember,
};
use time::OffsetDateTime;

use super::{IdentityError, IdentityService, RefreshOptions, RefreshReport, job::RefreshFlight};

impl IdentityService {
    pub async fn refresh(
        &self,
        options: RefreshOptions,
        fetched_at: OffsetDateTime,
    ) -> Result<RefreshReport, IdentityError> {
        self.refresh_with_owned_client(Arc::clone(&self.napcat), options, fetched_at)
            .await
    }

    pub async fn refresh_with_client(
        &self,
        client: &NapCatClient,
        options: RefreshOptions,
        fetched_at: OffsetDateTime,
    ) -> Result<RefreshReport, IdentityError> {
        self.refresh_with_owned_client(Arc::new(client.clone()), options, fetched_at)
            .await
    }

    async fn refresh_with_owned_client(
        &self,
        client: Arc<NapCatClient>,
        options: RefreshOptions,
        fetched_at: OffsetDateTime,
    ) -> Result<RefreshReport, IdentityError> {
        self.initialize_identity_jobs(fetched_at).await?;
        match self
            .begin_refresh_flight(client, options, fetched_at)
            .await?
        {
            RefreshFlight::Follower(receiver) => self.wait_for_refresh(receiver).await,
            RefreshFlight::Leader { id, receiver } => {
                let mut result = self.wait_for_refresh(receiver).await;
                self.reap_direct_refresh(id).await?;
                if let Ok(report) = &mut result {
                    report.performed = true;
                }
                result
            }
        }
    }

    pub(crate) async fn perform_refresh(
        &self,
        client: &NapCatClient,
        options: RefreshOptions,
        fetched_at: OffsetDateTime,
        job_generation: Option<u64>,
    ) -> Result<RefreshReport, IdentityError> {
        let friends = client
            .get_friend_list()
            .await?
            .into_data()
            .into_iter()
            .map(|friend| {
                Ok(IdentitySnapshotMember {
                    qq: remote_qq(friend.user_id())?,
                    nickname: friend.nickname().map(str::to_owned),
                    card: None,
                })
            })
            .collect::<Result<Vec<_>, IdentityError>>()?;
        let mut unique_users = friends
            .iter()
            .map(|friend| friend.qq.clone())
            .collect::<BTreeSet<_>>();

        let mut remote_groups = client.get_group_list().await?.into_data();
        if let Some(limit) = options.max_groups {
            remote_groups.truncate(limit.get());
        }
        let group_count = remote_groups.len();
        let mut progress = IdentityJobProgress {
            processed_groups: 0,
            total_groups: Some(group_count as u64),
            friend_count: Some(friends.len() as u64),
            current_group_id: None,
            current_group_name: None,
            unique_users: Some(unique_users.len() as u64),
        };
        self.persist_progress(
            job_generation,
            "已拉取好友和群列表，开始拉取群成员。",
            &progress,
        )
        .await?;

        let mut groups = Vec::with_capacity(group_count);
        for (index, group) in remote_groups.into_iter().enumerate() {
            let group_id = remote_group(group.group_id())?;
            let members = client
                .get_group_member_list(group.group_id(), options.no_cache.get())
                .await?
                .into_data()
                .into_iter()
                .map(|member| {
                    Ok(IdentitySnapshotMember {
                        qq: remote_qq(member.user_id())?,
                        nickname: member.nickname().map(str::to_owned),
                        card: member.card().map(str::to_owned),
                    })
                })
                .collect::<Result<Vec<_>, IdentityError>>()?;
            unique_users.extend(members.iter().map(|member| member.qq.clone()));
            let group_name = group.group_name().map(str::to_owned);
            progress.processed_groups = (index + 1) as u64;
            progress.current_group_id = Some(group_id.as_str().to_owned());
            progress.current_group_name = group_name.clone();
            progress.unique_users = Some(unique_users.len() as u64);
            let label = group_name.as_deref().unwrap_or_else(|| group_id.as_str());
            let message = format!("已刷新群 {}/{}：{label}", index + 1, group_count);
            self.persist_progress(job_generation, &message, &progress)
                .await?;
            groups.push(IdentityGroupSnapshot {
                group_id,
                group_name,
                member_count: group.member_count(),
                members,
            });
            if index + 1 < group_count && !options.group_delay.get().is_zero() {
                tokio::time::sleep(options.group_delay.get()).await;
            }
        }

        let metadata = self
            .directory
            .store()
            .replace_identity_snapshot(&IdentitySnapshot {
                fetched_at,
                friends,
                groups,
            })
            .await?;
        Ok(RefreshReport {
            performed: true,
            metadata,
        })
    }

    async fn persist_progress(
        &self,
        generation: Option<u64>,
        message: &str,
        progress: &IdentityJobProgress,
    ) -> Result<(), IdentityError> {
        if let Some(generation) = generation {
            self.directory
                .store()
                .update_identity_job_progress(generation, message, progress)
                .await?;
        }
        Ok(())
    }
}

fn remote_qq(value: &str) -> Result<QqId, IdentityError> {
    QqId::new(value).map_err(|_| IdentityError::InvalidRemoteIdentity { field: "QQ" })
}

fn remote_group(value: &str) -> Result<GroupId, IdentityError> {
    GroupId::new(value).map_err(|_| IdentityError::InvalidRemoteIdentity { field: "group_id" })
}
