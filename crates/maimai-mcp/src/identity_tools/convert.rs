use std::time::Duration;

use maimai_app::identity::{
    CacheStatus, GroupDelay, IdentityField, IdentityMatch, MaxGroups, NoCache, RefreshJobRequest,
    RefreshOptions, RefreshPolicy, Resolution,
};
use maimai_core::{GroupId, QqId};
use maimai_providers::{NapCatClient, NapCatConfig};
use maimai_storage::{
    IdentityGroupMembership, IdentityJob, IdentityJobStatus, IdentityRecord, IdentityStats,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    dto::{
        CacheDto, ErrorDto, GroupDto, IdentityDto, JobDto, MatchDto, ResolveCacheDto, ResolveDto,
        StatsDto,
    },
    error::IdentityToolError,
};

pub struct RefreshInput {
    pub request: RefreshJobRequest,
    pub client: Option<NapCatClient>,
}

pub fn refresh_input(
    args: super::dto::RefreshArgs,
    default: &NapCatConfig,
) -> Result<RefreshInput, IdentityToolError> {
    let base_url_override = args
        .napcat_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let timeout = args.timeout_ms.map_or(Ok(default.timeout()), |value| {
        if !(1_000..=60_000).contains(&value) {
            return Err(IdentityToolError::invalid(
                "timeoutMs 必须是 1000 到 60000 之间的整数。",
            ));
        }
        Ok(Duration::from_millis(value))
    })?;
    let group_delay = GroupDelay::new(Duration::from_millis(args.group_delay_ms.unwrap_or(250)))
        .map_err(|_| IdentityToolError::invalid("groupDelayMs 必须是 0 到 10000 之间的整数。"))?;
    let max_groups = args
        .max_groups
        .map(MaxGroups::new)
        .transpose()
        .map_err(|_| IdentityToolError::invalid("maxGroups 必须是正整数。"))?;
    let options = RefreshOptions {
        no_cache: NoCache::new(args.no_cache.unwrap_or(true)),
        group_delay,
        max_groups,
    };
    let policy = if args.force_refresh {
        RefreshPolicy::Force
    } else {
        RefreshPolicy::IfStale
    };
    if let Some(value) = base_url_override {
        let url = value
            .parse()
            .map_err(|_| IdentityToolError::invalid("napcatBaseUrl 格式不正确。"))?;
        let requested = NapCatConfig::new(url, timeout, None)
            .map_err(|_| IdentityToolError::invalid("NapCat 配置不正确。"))?;
        if requested.base_url() != default.base_url() {
            return Err(IdentityToolError::invalid(
                "napcatBaseUrl 只允许使用进程启动时配置的地址。",
            ));
        }
    }
    let client = if args.timeout_ms.is_some() {
        let config = NapCatConfig::new(
            default.base_url().clone(),
            timeout,
            default.access_token().cloned(),
        )
        .map_err(|_| IdentityToolError::invalid("NapCat 配置不正确。"))?;
        Some(
            NapCatClient::new(config)
                .map_err(|_| IdentityToolError::invalid("NapCat 配置不正确。"))?,
        )
    } else {
        None
    };
    Ok(RefreshInput {
        request: RefreshJobRequest { policy, options },
        client,
    })
}

pub fn group_id(value: Option<String>) -> Result<Option<GroupId>, IdentityToolError> {
    value
        .map(GroupId::new)
        .transpose()
        .map_err(|_| IdentityToolError::invalid("groupId 格式不正确。"))
}

pub fn qq(value: String) -> Result<QqId, IdentityToolError> {
    QqId::new(value).map_err(|_| IdentityToolError::invalid("必须提供 qq。"))
}

pub fn cache_dto(
    cache: &CacheStatus,
    job: Option<&IdentityJob>,
) -> Result<CacheDto, IdentityToolError> {
    Ok(CacheDto {
        cache_exists: cache.exists,
        fresh: cache.fresh,
        age_seconds: cache.age_seconds,
        daily_reset_hour_utc: cache.reset_hour_utc.get(),
        fetched_at: optional_timestamp(cache.fetched_at)?,
        updated_at: optional_timestamp(cache.updated_at)?,
        stats: stats_dto(cache.stats),
        job: job.map(job_dto).transpose()?,
    })
}

pub fn job_dto(job: &IdentityJob) -> Result<JobDto, IdentityToolError> {
    let running = job.status == IdentityJobStatus::Running;
    Ok(JobDto {
        status: job.status.as_str().to_owned(),
        started_at: timestamp(job.started_at)?,
        finished_at: optional_timestamp(job.finished_at)?,
        refresh_reason: match job.refresh_reason {
            maimai_storage::IdentityRefreshReason::ForceRefresh => "forceRefresh",
            maimai_storage::IdentityRefreshReason::StaleOrMissing => "stale_or_missing",
            maimai_storage::IdentityRefreshReason::AutoDaily => "auto_daily",
        }
        .to_owned(),
        message: job.message.clone(),
        processed_groups: running.then_some(job.progress.processed_groups),
        total_groups: running.then_some(job.progress.total_groups).flatten(),
        friend_count: running.then_some(job.progress.friend_count).flatten(),
        current_group_id: running
            .then_some(job.progress.current_group_id.clone())
            .flatten(),
        current_group_name: running
            .then_some(job.progress.current_group_name.clone())
            .flatten(),
        unique_users: running.then_some(job.progress.unique_users).flatten(),
        stats: job.stats.map(stats_dto),
        error: job.error.as_ref().map(error_dto),
    })
}

pub fn resolve_dto(
    resolution: Resolution,
    cache: &CacheStatus,
) -> Result<ResolveDto, IdentityToolError> {
    Ok(ResolveDto {
        query: resolution.query,
        group_id: resolution.group_id.map(|value| value.as_str().to_owned()),
        matches: resolution.matches.into_iter().map(match_dto).collect(),
        ambiguous: resolution.ambiguous,
        cache: ResolveCacheDto {
            fetched_at: optional_timestamp(cache.fetched_at)?,
            stats: stats_dto(cache.stats),
        },
    })
}

pub fn identity_dto(identity: IdentityRecord) -> IdentityDto {
    IdentityDto {
        qq: identity.qq.as_str().to_owned(),
        qq_nickname: identity.qq_nickname,
        friend_nickname: identity.friend_nickname,
        preferred_group: identity.preferred_group.map(group_dto),
        groups: identity.groups.into_iter().map(group_dto).collect(),
        waterfish_nickname: identity.waterfish_nickname,
        waterfish_username: identity
            .waterfish_username
            .map(|value| value.as_str().to_owned()),
        waterfish_rating: identity.waterfish_rating,
        is_friend: identity.is_friend,
    }
}

fn match_dto(candidate: IdentityMatch) -> MatchDto {
    let mut fields = candidate
        .matched_fields
        .into_iter()
        .map(field_name)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    fields.sort();
    MatchDto {
        identity: identity_dto(candidate.identity),
        match_score: candidate.score,
        matched_fields: fields,
    }
}

fn group_dto(group: IdentityGroupMembership) -> GroupDto {
    GroupDto {
        group_id: group.group_id.as_str().to_owned(),
        group_name: group.group_name,
        group_nickname: group.group_nickname,
        card: group.card,
        nickname: group.nickname,
    }
}

fn field_name(field: IdentityField) -> &'static str {
    match field {
        IdentityField::Qq => "qq",
        IdentityField::QqNickname => "qqNickname",
        IdentityField::FriendNickname => "friendNickname",
        IdentityField::WaterfishNickname => "waterfishNickname",
        IdentityField::WaterfishUsername => "waterfishUsername",
        IdentityField::PreferredGroupNickname => "preferredGroup.groupNickname",
        IdentityField::PreferredGroupCard => "preferredGroup.card",
        IdentityField::PreferredGroupQqNickname => "preferredGroup.nickname",
        IdentityField::GroupNickname => "group.groupNickname",
        IdentityField::GroupCard => "group.card",
        IdentityField::GroupQqNickname => "group.nickname",
    }
}

fn stats_dto(stats: IdentityStats) -> StatsDto {
    StatsDto {
        friend_count: stats.friend_count,
        group_count: stats.group_count,
        group_member_rows: stats.group_member_rows,
        unique_users: stats.unique_users,
    }
}

fn error_dto(error: &maimai_storage::IdentityJobError) -> ErrorDto {
    ErrorDto {
        code: error.code.as_str().to_owned(),
        message: error.message.clone(),
        status: error.status,
        body: error.body.clone(),
    }
}

fn timestamp(value: OffsetDateTime) -> Result<String, IdentityToolError> {
    value
        .format(&Rfc3339)
        .map_err(|_| IdentityToolError::internal())
}

fn optional_timestamp(value: Option<OffsetDateTime>) -> Result<Option<String>, IdentityToolError> {
    value.map(timestamp).transpose()
}
