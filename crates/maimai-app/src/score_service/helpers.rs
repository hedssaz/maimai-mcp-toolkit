use maimai_catalog::{CatalogQuery, CatalogSnapshot, SongIdFilter};
use maimai_core::{PlayerSelector, SongIdNamespace, SongIdValue, SourceSongId};
use maimai_providers::lxns_score::LxnsSongId;

use crate::{oauth::OAuthSubject, scores::Lookup};

use super::{PlayerScoreServiceError, PlayerScoreServiceErrorCode};

pub(super) fn player_selector(lookup: &Lookup) -> PlayerSelector {
    match lookup {
        Lookup::Qq(value) => PlayerSelector::Qq(value.clone()),
        Lookup::Username(value) => PlayerSelector::Username(value.clone()),
    }
}

pub(super) fn oauth_subject(lookup: &Lookup) -> Result<OAuthSubject, PlayerScoreServiceError> {
    let Lookup::Qq(qq) = lookup else {
        return Err(PlayerScoreServiceError::new(
            PlayerScoreServiceErrorCode::InvalidLookup,
            "LXNS 成绩只支持 QQ 查询",
        ));
    };
    OAuthSubject::new(qq.as_str()).map_err(PlayerScoreServiceError::oauth)
}

pub(super) fn lxns_song_id(
    snapshot: &CatalogSnapshot,
    source_id: &SourceSongId,
) -> Result<LxnsSongId, PlayerScoreServiceError> {
    let hits = snapshot
        .query(&CatalogQuery {
            id: Some(SongIdFilter::Exact(source_id.clone())),
            ..CatalogQuery::default()
        })
        .map_err(|_| {
            PlayerScoreServiceError::new(PlayerScoreServiceErrorCode::Catalog, "曲库 ID 查询失败")
        })?;
    let mut ids = hits
        .iter()
        .flat_map(|hit| &hit.music.source_ids)
        .filter_map(|id| match id.value() {
            SongIdValue::Numeric(value) if id.namespace() == SongIdNamespace::Lxns => Some(*value),
            SongIdValue::Numeric(_) | SongIdValue::Text(_) => None,
        })
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    match ids.as_slice() {
        [id] => LxnsSongId::new(*id).map_err(PlayerScoreServiceError::lxns),
        _ => Err(PlayerScoreServiceError::new(
            PlayerScoreServiceErrorCode::Catalog,
            "曲库没有唯一的 LXNS song id",
        )),
    }
}

pub(super) fn unsupported_source() -> PlayerScoreServiceError {
    PlayerScoreServiceError::new(
        PlayerScoreServiceErrorCode::UnsupportedSource,
        "official_cn 不是可查询的成绩来源",
    )
}
