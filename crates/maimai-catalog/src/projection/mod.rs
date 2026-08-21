mod chart;
mod source;
mod stats;

use std::collections::HashMap;

use maimai_core::{Music, SongIdNamespace, SongIdValue, SourceSongId};

use crate::{
    CatalogError, TextNormalizer,
    identity::SongIdentityIndex,
    metadata::CatalogMetadata,
    raw::{ChartStatsDocument, DivingFishSong, DxData, LxnsCatalog, OfficialDocument},
};

pub(crate) fn attach_source_projections(
    songs: &[Music],
    normalizer: &TextNormalizer,
    sources: ProjectionSources<'_>,
    stats: &ChartStatsDocument,
    metadata: &mut CatalogMetadata,
) -> Result<(), CatalogError> {
    let identities = SongIdentityIndex::build(songs, normalizer);
    let version_titles = sources
        .lxns
        .versions
        .iter()
        .map(|version| (version.version, version.title.as_str()))
        .collect::<HashMap<_, _>>();

    for raw in &sources.lxns.songs {
        if let Some(index) = identities.by_lxns(raw.id) {
            metadata.songs[index].source_projections.push(source::lxns(
                raw,
                &version_titles,
                stats,
            )?);
        }
    }
    for raw in sources.diving_fish {
        let id = source_id(SongIdNamespace::DivingFish, &raw.id, "Diving-Fish")?;
        let target = match id.value() {
            SongIdValue::Numeric(value) => {
                identities.by_diving_fish(*value, raw.chart_type.trim().eq_ignore_ascii_case("DX"))
            }
            SongIdValue::Text(_) => {
                identities.resolve_title("Diving-Fish", &raw.id, &raw.title, normalizer)?
            }
        };
        if let Some(index) = target {
            metadata.songs[index]
                .source_projections
                .push(source::diving_fish(raw, id, stats)?);
        }
    }
    for raw in &sources.dxdata.songs {
        if normalizer.normalize(&raw.title).is_empty() && raw.song_id.trim().is_empty() {
            continue;
        }
        if let Some(index) = identities.resolve_dx(raw, normalizer)? {
            metadata.songs[index]
                .source_projections
                .push(source::dxdata(raw, stats)?);
        }
    }
    for raw in &sources.official.songs {
        let target = if let Some(index) = identities.by_official(raw.id) {
            Some(index)
        } else {
            identities.resolve_title("official", &raw.id.to_string(), &raw.title, normalizer)?
        };
        if let Some(index) = target {
            metadata.songs[index]
                .source_projections
                .push(source::official(raw, stats)?);
            metadata.songs[index]
                .source_labels
                .insert(crate::metadata::SourceKind::Official);
        }
    }
    Ok(())
}

pub(crate) struct ProjectionSources<'a> {
    pub(crate) lxns: &'a LxnsCatalog,
    pub(crate) diving_fish: &'a [DivingFishSong],
    pub(crate) dxdata: &'a DxData,
    pub(crate) official: &'a OfficialDocument,
}

pub(super) fn source_id(
    namespace: SongIdNamespace,
    raw: &str,
    source_name: &'static str,
) -> Result<SourceSongId, CatalogError> {
    let trimmed = raw.trim();
    let value = match trimmed.parse::<u32>() {
        Ok(value) => SongIdValue::Numeric(value),
        Err(_) => SongIdValue::text(trimmed).map_err(|_| CatalogError::InvalidSourceSongId {
            source_name,
            value: raw.to_owned(),
        })?,
    };
    Ok(SourceSongId::new(namespace, value))
}
