mod chart;
mod fit;

use maimai_app::scores::{
    B50Chart, B50Result, Lookup, PlayerScores, RatingMode, SelectionReason, SourceSelection,
};
use maimai_catalog::PlateServer;
use maimai_core::ScoreSource;
use maimai_providers::RawJsonPayload;
use maimai_storage::{IdentityGroupMembership, IdentityRecord};
use serde_json::{Value, json};

use super::error::ScoreQueryToolError;
use chart::charts;
use fit::fit_index;

pub struct OutputContext {
    pub requested_at: String,
    pub identity: Option<IdentityRecord>,
    pub selection: SourceSelection,
    pub include_chart_metadata: bool,
    pub raw: Option<RawJsonPayload>,
}

pub struct PlateFilterMetadata {
    pub name: String,
    pub server: PlateServer,
    pub song_count: usize,
}

pub fn b50(result: &B50Result, context: OutputContext) -> Result<Value, ScoreQueryToolError> {
    let source = source_name(result.source);
    let mut output = json!({
        "source": source,
        "endpoint": endpoint(result.source, result.mode),
        "requestedAt": context.requested_at,
        "lookup": lookup(&result.lookup),
        "player": player(&result.player),
        "identity": context.identity.map(identity),
        "counts": {"sd":result.b35.len(),"dx":result.b15.len(),"total":result.total_count()},
        "ratingBreakdown": {
            "sd":result.rating_breakdown.b35,
            "dx":result.rating_breakdown.b15,
            "total":result.rating_breakdown.total,
        },
        "charts": {
            "sd": charts(&result.b35, context.include_chart_metadata)?,
            "dx": charts(&result.b15, context.include_chart_metadata)?,
        },
        "chartMetadata": chart_metadata(result, context.include_chart_metadata),
        "sourcePreference": {
            "qq": qq(&result.lookup),
            "preferredSource": source_key(context.selection.preferred_source),
            "preferredSourceLabel": source_label(context.selection.preferred_source),
            "usedSource": source_key(context.selection.source),
            "usedSourceLabel": source_label(context.selection.source),
            "explicitSource": context.selection.reason == SelectionReason::ExplicitOverride,
        },
    });
    if result.mode == RatingMode::Fit {
        output["computedB50"] = computed(result);
    }
    if context.include_chart_metadata {
        output["fitIndex"] = fit_index(result.fit_index)?;
    }
    if let Some(raw) = context.raw {
        output["raw"] = raw
            .into_value()
            .map_err(|_| ScoreQueryToolError::internal())?;
    }
    Ok(output)
}

pub fn records(
    scores: &PlayerScores,
    records: &[B50Chart],
    total: usize,
    plate: Option<PlateFilterMetadata>,
    context: OutputContext,
) -> Result<Value, ScoreQueryToolError> {
    let mut output = json!({
        "source": source_name(scores.source),
        "endpoint": records_endpoint(scores.source),
        "lookup": lookup(&scores.lookup),
        "requestedAt": context.requested_at,
        "identity": context.identity.map(identity),
        "player": player(&scores.player),
        "counts": {"total":total,"filtered":records.len()},
        "records": charts(records, context.include_chart_metadata)?,
        "sourcePreference": {
            "qq": qq(&scores.lookup),
            "preferredSource": source_key(context.selection.preferred_source),
            "preferredSourceLabel": source_label(context.selection.preferred_source),
            "usedSource": source_key(context.selection.source),
            "usedSourceLabel": source_label(context.selection.source),
            "explicitSource": context.selection.reason == SelectionReason::ExplicitOverride,
        }
    });
    if let Some(plate) = plate {
        output["plate"] = json!({
            "name":plate.name,
            "server":match plate.server { PlateServer::Cn => "cn", PlateServer::Jp => "jp", PlateServer::Custom => "custom" },
            "songCount":plate.song_count,
        });
    }
    if let Some(raw) = context.raw {
        output["raw"] = raw
            .into_value()
            .map_err(|_| ScoreQueryToolError::internal())?;
    }
    Ok(output)
}

pub fn song_scores(
    scores: &PlayerScores,
    music_id: Value,
    context: OutputContext,
) -> Result<Value, ScoreQueryToolError> {
    let records = charts(&scores.records, context.include_chart_metadata)?;
    let mut output = json!({
        "source":source_name(scores.source),
        "operation":song_operation(scores.source),
        "endpoint":song_endpoint(scores.source),
        "status":200,
        "url":null,
        "lookup":lookup(&scores.lookup),
        "identity":context.identity.map(identity),
        "player":player(&scores.player),
        "musicId":music_id,
        "requestedAt":context.requested_at,
        "record":records.as_array().and_then(|items| items.first()).cloned(),
        "records":records,
    });
    if let Some(raw) = context.raw {
        output["raw"] = raw
            .into_value()
            .map_err(|_| ScoreQueryToolError::internal())?;
    }
    Ok(output)
}

fn lookup(value: &Lookup) -> Value {
    match value {
        Lookup::Qq(value) => json!({"qq":value.as_str()}),
        Lookup::Username(value) => json!({"username":value.as_str()}),
    }
}

fn qq(value: &Lookup) -> Option<&str> {
    match value {
        Lookup::Qq(value) => Some(value.as_str()),
        Lookup::Username(_) => None,
    }
}

fn player(value: &maimai_app::scores::PlayerScoreProfile) -> Value {
    json!({
        "nickname":value.nickname,
        "username":value.username,
        "rating":value.rating,
        "actualRating":value.actual_rating,
        "additionalRating":value.additional_rating,
        "plate":value.plate,
    })
}

fn identity(value: IdentityRecord) -> Value {
    json!({
        "qq":value.qq.as_str(),
        "qqNickname":value.qq_nickname,
        "friendNickname":value.friend_nickname,
        "preferredGroup":value.preferred_group.map(group),
        "groups":value.groups.into_iter().map(group).collect::<Vec<_>>(),
        "waterfishNickname":value.waterfish_nickname,
        "waterfishUsername":value.waterfish_username.map(|value|value.to_string()),
        "waterfishRating":value.waterfish_rating,
        "isFriend":value.is_friend,
    })
}

fn group(value: IdentityGroupMembership) -> Value {
    json!({
        "groupId":value.group_id.as_str(),
        "groupName":value.group_name,
        "groupNickname":value.group_nickname,
        "card":value.card,
        "nickname":value.nickname,
    })
}

fn computed(result: &B50Result) -> Value {
    let mut versions = result
        .b15
        .iter()
        .map(|chart| chart.version.clone())
        .filter(|version| !version.is_empty())
        .collect::<Vec<_>>();
    versions.sort();
    versions.dedup();
    let stats = result.computation.unwrap_or_default();
    json!({
        "source":"playerRecords",
        "ratingSource":"catalog fitDiff",
        "versionSource":"latest diving-fish basic_info.from",
        "currentVersions":versions,
        "recordCount":stats.input,
        "eligibleCount":stats.eligible,
        "sdCandidateCount":result.b35.len(),
        "dxCandidateCount":result.b15.len(),
        "skipped":{
            "nonScoreType":stats.skipped_utage,
            "missingRating":stats.skipped_missing_rating,
            "missingFitDiff":stats.skipped_missing_fit,
            "duplicateLowerRa":stats.duplicate_lower_rating,
        },
        "actualRating":result.player.actual_rating,
        "computedRating":result.rating_breakdown.total,
    })
}

fn chart_metadata(result: &B50Result, include: bool) -> Value {
    if !include {
        return json!({"source":"catalog","skipped":true,"available":false,
            "matched":0,"missing":result.total_count()});
    }
    let matched = result
        .b35
        .iter()
        .chain(&result.b15)
        .filter(|chart| chart.fit_constant.is_some())
        .count();
    json!({"source":"catalog","available":true,"requested":result.total_count(),
        "matched":matched,"missing":result.total_count()-matched})
}

fn source_name(value: ScoreSource) -> &'static str {
    match value {
        ScoreSource::DivingFish => "diving-fish",
        ScoreSource::Lxns => "lxns",
        ScoreSource::Local => "local-maimai-db",
        ScoreSource::OfficialCn => "official-cn",
    }
}

fn source_key(value: ScoreSource) -> &'static str {
    match value {
        ScoreSource::DivingFish => "sy",
        ScoreSource::Lxns => "lxns",
        ScoreSource::Local => "local",
        ScoreSource::OfficialCn => "official_cn",
    }
}

fn source_label(value: ScoreSource) -> &'static str {
    match value {
        ScoreSource::DivingFish => "水鱼",
        ScoreSource::Lxns => "落雪",
        ScoreSource::Local => "本地缓存",
        ScoreSource::OfficialCn => "官服",
    }
}

fn endpoint(source: ScoreSource, mode: RatingMode) -> &'static str {
    match (source, mode) {
        (ScoreSource::DivingFish, RatingMode::Actual) => "/query/player",
        (ScoreSource::DivingFish, RatingMode::Fit) => "/dev/player/records",
        (ScoreSource::Lxns, RatingMode::Actual) => "/user/maimai/player/bests",
        (ScoreSource::Lxns, RatingMode::Fit) => "/user/maimai/player/scores",
        (ScoreSource::Local, _) => "local://player/records",
        (ScoreSource::OfficialCn, _) => "official-cn://records",
    }
}

fn records_endpoint(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "/dev/player/records",
        ScoreSource::Lxns => "/user/maimai/player/scores",
        ScoreSource::Local => "local://player/records",
        ScoreSource::OfficialCn => "official-cn://records",
    }
}

fn song_operation(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "maimai_dev_player_records_get",
        ScoreSource::Lxns => "lxns_player_song_bests_get",
        ScoreSource::Local => "local_maimai_player_records_filter",
        ScoreSource::OfficialCn => "official_cn_player_records_filter",
    }
}

fn song_endpoint(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "/dev/player/records",
        ScoreSource::Lxns => "/user/maimai/player/bests",
        ScoreSource::Local => "local://player/records",
        ScoreSource::OfficialCn => "official-cn://records",
    }
}
