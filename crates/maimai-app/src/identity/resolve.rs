use std::collections::BTreeSet;

use maimai_core::{GroupId, QqId};
use maimai_storage::{IdentityGroupMembership, IdentityRecord};

use super::{
    IdentityDirectory, IdentityError, IdentityField, IdentityMatch, IdentityQuery, IdentityService,
    MaxResults, Resolution,
};

impl IdentityService {
    pub async fn get_identity(
        &self,
        qq: &QqId,
        preferred_group: Option<&GroupId>,
    ) -> Result<Option<IdentityRecord>, IdentityError> {
        self.directory.get_identity(qq, preferred_group).await
    }

    pub async fn resolve_identity(
        &self,
        query: &IdentityQuery,
        preferred_group: Option<&GroupId>,
        max_results: MaxResults,
    ) -> Result<Resolution, IdentityError> {
        self.directory
            .resolve_identity(query, preferred_group, max_results)
            .await
    }
}

pub(super) async fn resolve(
    directory: &IdentityDirectory,
    query: &IdentityQuery,
    preferred_group: Option<&GroupId>,
    max_results: MaxResults,
) -> Result<Resolution, IdentityError> {
    let raw_query = query.as_str();
    let normalized_query = normalize(raw_query);
    let mut matches = directory
        .store()
        .identities(preferred_group)
        .await?
        .into_iter()
        .filter_map(|identity| score_identity(identity, raw_query, &normalized_query))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.identity.qq.cmp(&right.identity.qq))
    });
    matches.truncate(max_results.get());
    let exact_count = matches
        .iter()
        .filter(|candidate| candidate.score >= 100)
        .count();
    let tied_first = matches
        .first()
        .zip(matches.get(1))
        .is_some_and(|(first, second)| first.score == second.score);
    Ok(Resolution {
        query: raw_query.to_owned(),
        group_id: preferred_group.cloned(),
        ambiguous: exact_count > 1 || tied_first,
        matches,
    })
}

fn score_identity(
    identity: IdentityRecord,
    raw_query: &str,
    normalized_query: &str,
) -> Option<IdentityMatch> {
    if identity.qq.as_str() == raw_query {
        return Some(IdentityMatch {
            identity,
            score: 200,
            matched_fields: vec![IdentityField::Qq],
        });
    }

    let mut score = 0_u16;
    let mut fields = BTreeSet::new();
    score_direct(
        identity.qq_nickname.as_deref(),
        IdentityField::QqNickname,
        normalized_query,
        &mut score,
        &mut fields,
    );
    score_direct(
        identity.friend_nickname.as_deref(),
        IdentityField::FriendNickname,
        normalized_query,
        &mut score,
        &mut fields,
    );
    score_direct(
        identity.waterfish_nickname.as_deref(),
        IdentityField::WaterfishNickname,
        normalized_query,
        &mut score,
        &mut fields,
    );
    score_direct(
        identity
            .waterfish_username
            .as_ref()
            .map(|username| username.as_str()),
        IdentityField::WaterfishUsername,
        normalized_query,
        &mut score,
        &mut fields,
    );
    if let Some(group) = identity.preferred_group.as_ref() {
        score_group(group, normalized_query, 10, true, &mut score, &mut fields);
    }
    for group in &identity.groups {
        score_group(group, normalized_query, 0, false, &mut score, &mut fields);
    }
    (score > 0).then(|| IdentityMatch {
        identity,
        score,
        matched_fields: fields.into_iter().collect(),
    })
}

fn score_direct(
    value: Option<&str>,
    field: IdentityField,
    query: &str,
    score: &mut u16,
    fields: &mut BTreeSet<IdentityField>,
) {
    let field_score = score_name(value, query);
    if field_score > 0 {
        *score = (*score).max(field_score);
        fields.insert(field);
    }
}

fn score_group(
    group: &IdentityGroupMembership,
    query: &str,
    bonus: u16,
    preferred: bool,
    score: &mut u16,
    fields: &mut BTreeSet<IdentityField>,
) {
    let candidates = [
        (
            Some(group.group_nickname.as_str()),
            if preferred {
                IdentityField::PreferredGroupNickname
            } else {
                IdentityField::GroupNickname
            },
        ),
        (
            group.card.as_deref(),
            if preferred {
                IdentityField::PreferredGroupCard
            } else {
                IdentityField::GroupCard
            },
        ),
        (
            group.nickname.as_deref(),
            if preferred {
                IdentityField::PreferredGroupQqNickname
            } else {
                IdentityField::GroupQqNickname
            },
        ),
    ];
    for (value, field) in candidates {
        let field_score = score_name(value, query);
        if field_score > 0 {
            *score = (*score).max(field_score + bonus);
            fields.insert(field);
        }
    }
}

fn score_name(value: Option<&str>, query: &str) -> u16 {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return 0;
    };
    let normalized = normalize(value);
    if normalized == query {
        100
    } else if normalized.contains(query) {
        50
    } else {
        0
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}
