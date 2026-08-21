use std::{cmp::Reverse, collections::HashMap};

use maimai_catalog::{CatalogSnapshot, PlateMember, PlateMembers, PlateQuery, PlateServer};
use maimai_core::{ChartKey, Difficulty, SongIdValue};
use maimai_render::{PlateChartState, PlateMemberView, PlateTableKind, PlateTableView};

use crate::scores::{B50Chart, PlayerScores, best_by_chart};

use super::{CompletionError, CompletionTarget, PlateSpec, records};

pub(crate) struct PreparedPlate {
    pub(crate) version: String,
    pub(crate) target: CompletionTarget,
    pub(crate) server: PlateServer,
    pub(crate) view: PlateTableView,
}

pub(crate) fn prepare(
    snapshot: &CatalogSnapshot,
    scores: &PlayerScores,
    spec: &PlateSpec,
    allow_jp: bool,
) -> Result<PreparedPlate, CompletionError> {
    let server = resolve_server(snapshot, spec, allow_jp)?;
    let query = PlateQuery::new(spec.version.clone(), server);
    let members = snapshot.plate_members(&query);
    if members.is_empty() {
        return Err(CompletionError::UnknownPlate(
            spec.version.as_str().to_owned(),
        ));
    }
    let records = best_by_chart(&scores.records);
    let mut views = members
        .members()
        .iter()
        .map(|member| {
            member_view(member, spec.target, &records).map(|view| (member.master_constant(), view))
        })
        .collect::<Result<Vec<_>, _>>()?;
    views.sort_by_key(|value| Reverse(value.0));
    let views = views.into_iter().map(|(_, view)| view).collect();
    let version = normalized_version(spec.version.as_str(), server);
    Ok(PreparedPlate {
        version: version.clone(),
        target: spec.target,
        server,
        view: PlateTableView {
            kind: match server {
                PlateServer::Cn => PlateTableKind::China,
                PlateServer::Jp => PlateTableKind::Japan,
                PlateServer::Custom => PlateTableKind::Custom,
            },
            version,
            target: spec.target.label().to_owned(),
            declared_song_count: members.declared_song_count(),
            members: views,
        },
    })
}

pub(crate) fn resolved_members(
    snapshot: &CatalogSnapshot,
    spec: &PlateSpec,
    allow_jp: bool,
) -> Result<(PlateServer, PlateMembers), CompletionError> {
    let server = resolve_server(snapshot, spec, allow_jp)?;
    let query = PlateQuery::new(spec.version.clone(), server);
    let members = snapshot.plate_members(&query);
    if members.is_empty() {
        return Err(CompletionError::UnknownPlate(
            spec.version.as_str().to_owned(),
        ));
    }
    Ok((server, members))
}

fn resolve_server(
    snapshot: &CatalogSnapshot,
    spec: &PlateSpec,
    allow_jp: bool,
) -> Result<PlateServer, CompletionError> {
    if spec.server == Some(PlateServer::Jp) && !allow_jp {
        return Err(CompletionError::invalid(
            "当前 surface 不支持日服/dxdata 曲目数据。",
        ));
    }
    if let Some(server) = spec.server {
        return Ok(server);
    }
    if snapshot.plate_exists(&spec.version, PlateServer::Cn) {
        return Ok(PlateServer::Cn);
    }
    if allow_jp && snapshot.plate_exists(&spec.version, PlateServer::Jp) {
        return Ok(PlateServer::Jp);
    }
    if snapshot.plate_exists(&spec.version, PlateServer::Custom) {
        return Ok(PlateServer::Custom);
    }
    Err(CompletionError::UnknownPlate(
        spec.version.as_str().to_owned(),
    ))
}

fn member_view(
    member: &PlateMember,
    target: CompletionTarget,
    records: &HashMap<ChartKey, &B50Chart>,
) -> Result<PlateMemberView, CompletionError> {
    let charts = member
        .charts()
        .iter()
        .filter(|chart| chart.difficulty() != Difficulty::Utage)
        .map(|chart| {
            let record = chart.key().and_then(|key| records.get(key).copied());
            Ok(PlateChartState {
                difficulty: chart.difficulty(),
                state: records::state(record, target)?,
            })
        })
        .collect::<Result<_, CompletionError>>()?;
    let master_level = member
        .charts()
        .iter()
        .find(|chart| chart.difficulty() == Difficulty::Master)
        .map_or_else(String::new, |chart| chart.level().to_owned());
    Ok(PlateMemberView {
        cover_id: member.identity().display_id().clone(),
        image_name: member.image_name().map(str::to_owned),
        title: member.title().to_owned(),
        generation: member.generation(),
        master_level,
        charts,
    })
}

fn normalized_version(value: &str, server: PlateServer) -> String {
    let value = match value.trim() {
        "晓" => "暁",
        "樱" => "櫻",
        "堇" => "菫",
        "辉" => "輝",
        "华" => "華",
        value => value,
    };
    if server == PlateServer::Jp
        && matches!(
            value.replace([' ', '　'], "").to_ascii_lowercase().as_str(),
            "circle" | "maimaiでらっくすcircle"
        )
    {
        "丸".to_owned()
    } else {
        value.to_owned()
    }
}

pub(crate) fn display_id(value: &SongIdValue) -> String {
    match value {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}
