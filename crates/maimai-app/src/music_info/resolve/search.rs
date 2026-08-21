use maimai_catalog::{
    CatalogQuery, CatalogSnapshot, MatchKind, SearchHit, SongIdFilter, SourceKind,
};
use maimai_core::SongIdValue;

use super::super::{MusicInfoChartType, MusicInfoError, MusicInfoRequest, model::song_id_text};

pub(super) fn select_hit<'a>(
    snapshot: &'a CatalogSnapshot,
    request: &MusicInfoRequest,
) -> Result<Option<(SearchHit<'a>, Option<MusicInfoChartType>)>, MusicInfoError> {
    if let Some(resolved) = &request.resolved {
        if let Some(id) = &resolved.id
            && let Some(hit) = unique_id(snapshot, id)?
        {
            return Ok(Some((hit, inferred_from_music_id(request))));
        }
        if let Some(title) = resolved.title.as_deref()
            && let Some(hit) = exact_or_unique_text(snapshot, title, None)?
        {
            return Ok(Some((hit, inferred_from_music_id(request))));
        }
    }
    if let Some(id) = &request.music_id
        && let Some(hit) = unique_id(snapshot, id)?
    {
        return Ok(Some((hit, inferred_from_music_id(request))));
    }
    if let Some(id) = request
        .music_id
        .as_ref()
        .and_then(|id| alternate_chart_id(id, request.chart_type))
        && let Some(hit) = unique_id(snapshot, &id)?
    {
        return Ok(Some((hit, request.chart_type)));
    }
    let Some(query) = request.query.as_deref() else {
        return Ok(None);
    };
    let direct = text_hits(snapshot, query, None)?;
    let exact = direct
        .iter()
        .filter(|hit| exact_kind(hit.matched_by))
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        let hit = exact[0].clone();
        let inferred = if identity_exact(hit.matched_by) {
            None
        } else {
            infer_query_type(query).map(|(chart_type, _)| chart_type)
        };
        return Ok(Some((hit, inferred)));
    }
    if exact.len() > 1 {
        return Err(ambiguous(&direct));
    }
    if let Some((chart_type, stripped)) = infer_query_type(query) {
        return exact_or_unique_text(snapshot, &stripped, Some(chart_type))
            .map(|value| value.map(|hit| (hit, Some(chart_type))));
    }
    match direct.len() {
        0 => Ok(None),
        1 => Ok(direct.into_iter().next().map(|hit| (hit, None))),
        _ => Err(ambiguous(&direct)),
    }
}

fn unique_id<'a>(
    snapshot: &'a CatalogSnapshot,
    id: &SongIdValue,
) -> Result<Option<SearchHit<'a>>, MusicInfoError> {
    let hits = snapshot
        .query(&CatalogQuery {
            id: Some(SongIdFilter::AnySource(id.clone())),
            limit: Some(6),
            ..CatalogQuery::default()
        })
        .map_err(|error| MusicInfoError::invalid(error.to_string()))?;
    match hits.len() {
        0 => Ok(None),
        1 => Ok(hits.into_iter().next()),
        _ => Err(ambiguous(&hits)),
    }
}

fn exact_or_unique_text<'a>(
    snapshot: &'a CatalogSnapshot,
    query: &str,
    chart_type: Option<MusicInfoChartType>,
) -> Result<Option<SearchHit<'a>>, MusicInfoError> {
    let hits = text_hits(snapshot, query, chart_type)?;
    let exact = hits
        .iter()
        .filter(|hit| exact_kind(hit.matched_by))
        .cloned()
        .collect::<Vec<_>>();
    match (exact.len(), hits.len()) {
        (1, _) => Ok(exact.into_iter().next()),
        (0, 0) => Ok(None),
        (0, 1) => Ok(hits.into_iter().next()),
        _ => Err(ambiguous(&hits)),
    }
}

fn text_hits<'a>(
    snapshot: &'a CatalogSnapshot,
    query: &str,
    chart_type: Option<MusicInfoChartType>,
) -> Result<Vec<SearchHit<'a>>, MusicInfoError> {
    let mut catalog_query = CatalogQuery::text(query, 6);
    if let Some(chart_type) = chart_type {
        catalog_query.generations.insert(chart_type.generation());
    }
    snapshot
        .query(&catalog_query)
        .map_err(|error| MusicInfoError::invalid(error.to_string()))
}

fn inferred_from_music_id(request: &MusicInfoRequest) -> Option<MusicInfoChartType> {
    request.music_id.as_ref().and_then(inferred_from_value)
}

fn alternate_chart_id(
    value: &SongIdValue,
    chart_type: Option<MusicInfoChartType>,
) -> Option<SongIdValue> {
    let SongIdValue::Numeric(value) = value else {
        return None;
    };
    match chart_type {
        Some(MusicInfoChartType::Standard) if (10_001..100_000).contains(value) => {
            Some(SongIdValue::Numeric(value - 10_000))
        }
        Some(MusicInfoChartType::Deluxe) if *value < 10_000 => {
            value.checked_add(10_000).map(SongIdValue::Numeric)
        }
        _ => None,
    }
}

pub(super) fn inferred_from_value(value: &SongIdValue) -> Option<MusicInfoChartType> {
    matches!(value, SongIdValue::Numeric(value) if (10_001..100_000).contains(value))
        .then_some(MusicInfoChartType::Deluxe)
}

fn infer_query_type(query: &str) -> Option<(MusicInfoChartType, String)> {
    let trimmed = query.trim();
    for (chart_type, marker) in [
        (MusicInfoChartType::Standard, "standard"),
        (MusicInfoChartType::Standard, "标准谱面"),
        (MusicInfoChartType::Standard, "std"),
        (MusicInfoChartType::Standard, "st"),
        (MusicInfoChartType::Standard, "sd"),
        (MusicInfoChartType::Standard, "标准"),
        (MusicInfoChartType::Deluxe, "でらっくす"),
        (MusicInfoChartType::Deluxe, "dx"),
    ] {
        if let Some(value) = strip_ascii_case_prefix(trimmed, marker)
            .or_else(|| strip_ascii_case_suffix(trimmed, marker))
        {
            let value = value
                .trim_matches(|character: char| {
                    character.is_whitespace() || matches!(character, '-' | '_' | ':')
                })
                .to_owned();
            if !value.is_empty() {
                return Some((chart_type, value));
            }
        }
    }
    None
}

fn strip_ascii_case_prefix<'a>(value: &'a str, marker: &str) -> Option<&'a str> {
    value
        .get(..marker.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(marker))
        .and_then(|_| value.get(marker.len()..))
}

fn strip_ascii_case_suffix<'a>(value: &'a str, marker: &str) -> Option<&'a str> {
    let start = value.len().checked_sub(marker.len())?;
    value
        .get(start..)
        .filter(|suffix| suffix.eq_ignore_ascii_case(marker))
        .and_then(|_| value.get(..start))
}

pub(super) fn numeric_query(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    let trimmed = trimmed
        .get(..2)
        .filter(|prefix| prefix.eq_ignore_ascii_case("id"))
        .and_then(|_| trimmed.get(2..))
        .unwrap_or(trimmed)
        .trim();
    trimmed.parse().ok().filter(|value| *value > 0)
}

fn exact_kind(value: MatchKind) -> bool {
    matches!(
        value,
        MatchKind::NumericId | MatchKind::ExactTitle | MatchKind::ExactAlias
    )
}

fn identity_exact(value: MatchKind) -> bool {
    matches!(value, MatchKind::NumericId | MatchKind::ExactTitle)
}

fn ambiguous(hits: &[SearchHit<'_>]) -> MusicInfoError {
    let lines = hits
        .iter()
        .take(5)
        .enumerate()
        .map(|(index, hit)| {
            format!(
                "{}. {} | ID {} | {}",
                index + 1,
                hit.music.title,
                display_hit_id(hit),
                if hit.music.artist.is_empty() {
                    "-"
                } else {
                    &hit.music.artist
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    MusicInfoError::Ambiguous(lines)
}

fn display_hit_id(hit: &SearchHit<'_>) -> String {
    let mut values = hit
        .metadata
        .source_projections
        .iter()
        .filter(|source| source.source == SourceKind::DivingFish)
        .filter_map(|source| match source.id.value() {
            SongIdValue::Numeric(value) => Some(*value),
            SongIdValue::Text(_) => None,
        })
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    match values.as_slice() {
        [value] => value.to_string(),
        [first, second] => format!("ST#{first} / DX#{second}"),
        _ => song_id_text(hit.music.primary_id.value()),
    }
}
