use std::collections::{BTreeMap, BTreeSet, HashMap};

use maimai_core::{ChartGeneration, SourceSongId};

use crate::{
    CatalogError,
    raw::{DxData, PlateDocument},
};

use super::{
    PlateMember,
    catalog::MemberSet,
    source_index::{SourceIndex, member_from_projection, with_remaster},
};

pub(super) fn build_cn(
    document: PlateDocument,
    sources: &SourceIndex<'_>,
) -> Result<HashMap<String, MemberSet>, CatalogError> {
    let remaster_ids = document
        .content
        .get("舞ReMASTER")
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeSet<_>>();
    document
        .content
        .into_iter()
        .filter(|(name, _)| name != "舞ReMASTER")
        .map(|(name, ids)| {
            let declared = ids.len();
            let mut members = Vec::new();
            for id in ids {
                let Some((index, projection)) = sources.df(id)? else {
                    continue;
                };
                let member = member_from_projection(
                    sources.song(index),
                    projection,
                    Some(projection),
                    None,
                    None,
                )?;
                members.push(with_remaster(
                    member,
                    name == "舞" && remaster_ids.contains(&id),
                ));
            }
            Ok((name, MemberSet { declared, members }))
        })
        .collect()
}

pub(super) fn build_jp(
    dxdata: &DxData,
    cn: &HashMap<String, MemberSet>,
    sources: &SourceIndex<'_>,
) -> Result<HashMap<String, MemberSet>, CatalogError> {
    let mut result = old_frames(cn);
    let version_names = jp_versions();
    let mut by_version = HashMap::<String, Vec<PlateMember>>::new();
    let mut seen = HashMap::<String, BTreeSet<(SourceSongId, ChartGeneration)>>::new();
    for raw in &dxdata.songs {
        if raw.song_id.trim().is_empty() {
            continue;
        }
        let id = crate::projection::source_id(
            maimai_core::SongIdNamespace::DxRating,
            &raw.song_id,
            "dxdata",
        )?;
        let Some((index, projection)) = sources.dx(&id) else {
            continue;
        };
        let versions = projection
            .charts
            .iter()
            .filter_map(|chart| {
                version_names
                    .values()
                    .any(|version| *version == chart.version)
                    .then_some((chart.version.clone(), chart.generation))
            })
            .collect::<BTreeSet<_>>();
        for (version, generation) in versions {
            let key = (sources.song(index).primary_id.clone(), generation);
            if !seen.entry(version.clone()).or_default().insert(key) {
                continue;
            }
            let member = member_from_projection(
                sources.song(index),
                projection,
                sources.df_for_song_generation(index, generation)?,
                Some(&version),
                Some(generation),
            )?;
            by_version
                .entry(version)
                .or_default()
                .push(with_remaster(member, false));
        }
    }
    for (name, version) in version_names {
        let members = by_version.remove(version).unwrap_or_default();
        result.insert(
            name.to_owned(),
            MemberSet {
                declared: members.len(),
                members,
            },
        );
    }
    Ok(result)
}

fn old_frames(cn: &HashMap<String, MemberSet>) -> HashMap<String, MemberSet> {
    let mut result = HashMap::new();
    for (name, key) in [
        ("初", "真"),
        ("真", "真"),
        ("超", "超"),
        ("檄", "檄"),
        ("橙", "橙"),
        ("暁", "暁"),
        ("桃", "桃"),
        ("櫻", "櫻"),
        ("紫", "紫"),
        ("菫", "菫"),
        ("白", "白"),
        ("雪", "雪"),
        ("輝", "輝"),
        ("霸", "舞"),
        ("舞", "舞"),
    ] {
        if let Some(values) = cn.get(key) {
            result.insert(name.to_owned(), values.clone());
        }
    }
    result
}

pub(super) fn cn_key(name: &str) -> &str {
    match name {
        "晓" => "暁",
        "樱" => "櫻",
        "堇" => "菫",
        "辉" => "輝",
        "华" | "華" | "熊" => "熊&华",
        "爽" | "煌" => "爽&煌",
        "宙" | "星" => "宙&星",
        "祭" | "祝" => "祭&祝",
        "双" | "宴" => "双&宴",
        "霸" | "舞" => "舞",
        _ => name,
    }
}

pub(super) fn normalize_jp_name(name: &str) -> String {
    let compact = name.replace([' ', '　'], "").to_lowercase();
    if matches!(compact.as_str(), "circle" | "maimaiでらっくすcircle") {
        return "丸".to_owned();
    }
    match name {
        "晓" => "暁".to_owned(),
        "樱" => "櫻".to_owned(),
        "堇" => "菫".to_owned(),
        "辉" => "輝".to_owned(),
        "华" => "華".to_owned(),
        _ => name.to_owned(),
    }
}

fn jp_versions() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("熊", "maimaiでらっくす"),
        ("華", "maimaiでらっくす PLUS"),
        ("爽", "Splash"),
        ("煌", "Splash PLUS"),
        ("宙", "UNiVERSE"),
        ("星", "UNiVERSE PLUS"),
        ("祭", "FESTiVAL"),
        ("祝", "FESTiVAL PLUS"),
        ("双", "BUDDiES"),
        ("宴", "BUDDiES PLUS"),
        ("镜", "PRiSM"),
        ("彩", "PRiSM PLUS"),
        ("丸", "CiRCLE"),
    ])
}
