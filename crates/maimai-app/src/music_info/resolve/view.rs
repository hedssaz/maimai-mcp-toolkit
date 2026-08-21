use maimai_catalog::{
    CatalogSnapshot, SearchHit, SourceChartProjection, SourceKind, SourceSongProjection,
};
use maimai_core::{
    ChartConstant, ChartGeneration, Difficulty, SongIdNamespace, SongIdValue, SourceSongId,
};
use maimai_render::{MusicInfoChart, MusicInfoView};

use super::{
    PreparedMusicInfo,
    search::{inferred_from_value, numeric_query},
};
use crate::music_info::{
    MusicInfoChartType, MusicInfoError, MusicInfoRequest, model::song_id_text,
};

pub(super) fn prepare(
    snapshot: &CatalogSnapshot,
    request: &MusicInfoRequest,
    hit: &SearchHit<'_>,
    generation: ChartGeneration,
) -> Result<PreparedMusicInfo, MusicInfoError> {
    let mut projections = hit.metadata.source_projections.iter().collect::<Vec<_>>();
    projections.sort_by_key(|projection| projection.source);
    let primary = projections.first().copied();
    let title = primary.map_or(hit.music.title.as_str(), |value| value.title.as_str());
    let artist = primary.map_or(hit.music.artist.as_str(), |value| value.artist.as_str());
    let genre = primary.map_or(hit.music.genre.as_str(), |value| value.genre.as_str());
    let version = primary.map_or(hit.music.version.as_str(), |value| value.version.as_str());
    let is_new = primary
        .and_then(|value| value.is_new)
        .unwrap_or(hit.metadata.is_new_cn || hit.metadata.is_new_jp);
    let image_name = request
        .image_name
        .clone()
        .or_else(|| {
            request
                .resolved
                .as_ref()
                .and_then(|value| value.image_name.clone())
        })
        .or_else(|| {
            projections
                .iter()
                .find(|projection| projection.source == SourceKind::Japan)
                .and_then(|projection| projection.image_name.clone())
        });
    let (display_id, cover_id) = render_id(request, primary, &projections, generation);
    let music_id = display_id.map_or_else(
        || {
            cover_id
                .as_ref()
                .map_or_else(String::new, |id| song_id_text(id.value()))
        },
        |value| value.to_string(),
    );
    let mut charts = Vec::new();
    let mut chart_keys = Vec::new();
    for &difficulty in difficulties(generation) {
        let mut candidates = projections
            .iter()
            .flat_map(|source| {
                source
                    .charts
                    .iter()
                    .filter(move |chart| {
                        chart.generation == generation && chart.difficulty == difficulty
                    })
                    .map(move |chart| (*source, chart))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(source, _)| source.source);
        let Some((_, primary_chart)) = candidates.first().copied() else {
            continue;
        };
        let detail = candidates
            .iter()
            .map(|(_, chart)| *chart)
            .max_by_key(|chart| note_detail_score(chart))
            .unwrap_or(primary_chart);
        let constant = chart_constant(primary_chart.constant, "constant")?;
        let fit_constant = primary_chart
            .fit_stats
            .as_ref()
            .and_then(|stats| stats.fit_diff)
            .map(|value| ChartConstant::from_decimal_str(&value.to_string()))
            .transpose()
            .map_err(|_| MusicInfoError::invalid("拟合定数超出支持范围"))?;
        let charter = if primary_chart.note_designer.trim().is_empty()
            || primary_chart.note_designer.trim() == "-"
        {
            detail.note_designer.clone()
        } else {
            primary_chart.note_designer.clone()
        };
        charts.push(MusicInfoChart::new(
            difficulty,
            primary_chart.level.clone(),
            constant,
            fit_constant,
            detail.notes,
            primary_chart.note_total.or(detail.note_total),
            charter,
        )?);
        if let Some(chart) = hit.music.charts.iter().find(|chart| {
            chart.key.generation() == generation && chart.key.difficulty() == difficulty
        }) {
            chart_keys.push(chart.key.clone());
        }
    }
    let visual_generation = if matches!(
        generation,
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
    ) {
        ChartGeneration::Deluxe
    } else {
        generation
    };
    let view = MusicInfoView::new(
        cover_id,
        display_id,
        title,
        artist,
        genre,
        version,
        (hit.music.bpm > 0).then_some(hit.music.bpm),
        Some(visual_generation),
        is_new,
        image_name,
        charts,
    )?;
    let current = projections.iter().any(|projection| {
        projection.source == SourceKind::DivingFish
            && snapshot
                .current_diving_fish_versions()
                .contains(&projection.version)
            && projection
                .charts
                .iter()
                .any(|chart| chart.generation == generation)
    });
    Ok(PreparedMusicInfo {
        view,
        chart_keys,
        generation,
        current,
        chart_type: chart_type(generation),
        query: request.query_label(),
        music_id,
        title: title.to_owned(),
    })
}

pub(super) fn partial(request: &MusicInfoRequest) -> Result<PreparedMusicInfo, MusicInfoError> {
    let id = request.music_id.clone().or_else(|| {
        request
            .query
            .as_deref()
            .and_then(numeric_query)
            .map(SongIdValue::Numeric)
    });
    let image_name = request.image_name.clone().or_else(|| {
        request
            .resolved
            .as_ref()
            .and_then(|value| value.image_name.clone())
    });
    if id.is_none() && image_name.is_none() {
        return Err(MusicInfoError::NotFound(request.query_label()));
    }
    let inferred = request
        .chart_type
        .or_else(|| id.as_ref().and_then(inferred_from_value));
    let source_id = id.as_ref().map(|id| {
        SourceSongId::new(
            if matches!(id, SongIdValue::Text(_)) {
                SongIdNamespace::DxRating
            } else {
                SongIdNamespace::DivingFish
            },
            id.clone(),
        )
    });
    let display_id = match id.as_ref() {
        Some(SongIdValue::Numeric(value)) => Some(*value),
        Some(SongIdValue::Text(_)) | None => None,
    };
    let view = MusicInfoView::new(
        source_id,
        display_id,
        request.known.title.clone(),
        request.known.artist.clone(),
        request.known.genre.clone(),
        request.known.version.clone(),
        request.known.bpm,
        inferred.map(MusicInfoChartType::generation),
        request.known.is_new,
        image_name,
        Vec::new(),
    )?
    .require_cover(true);
    Ok(PreparedMusicInfo {
        view,
        chart_keys: Vec::new(),
        generation: inferred.map_or(ChartGeneration::Standard, MusicInfoChartType::generation),
        current: false,
        chart_type: inferred,
        query: request.query_label(),
        music_id: id.as_ref().map_or_else(String::new, song_id_text),
        title: request.known.title.clone(),
    })
}

fn render_id(
    request: &MusicInfoRequest,
    primary: Option<&SourceSongProjection>,
    projections: &[&SourceSongProjection],
    generation: ChartGeneration,
) -> (Option<u32>, Option<SourceSongId>) {
    if let Some(SongIdValue::Numeric(value)) = request.music_id {
        let value = normalized_explicit_id(value, generation);
        return (
            Some(value),
            primary.map(|source| SourceSongId::numeric(source.id.namespace(), value)),
        );
    }
    let source = projections
        .iter()
        .find(|source| {
            source.source == SourceKind::DivingFish
                && source
                    .charts
                    .iter()
                    .any(|chart| chart.generation == generation)
        })
        .copied()
        .or(primary);
    let Some(source) = source else {
        return (None, None);
    };
    let SongIdValue::Numeric(raw) = source.id.value() else {
        return (None, Some(source.id.clone()));
    };
    let raw = *raw;
    let display = match generation {
        ChartGeneration::Deluxe if raw < 10_000 => raw.checked_add(10_000),
        ChartGeneration::Standard
            if source.source == SourceKind::DivingFish && (10_001..100_000).contains(&raw) =>
        {
            Some(raw - 10_000)
        }
        _ => Some(raw),
    };
    (
        display,
        display.map(|value| SourceSongId::numeric(source.id.namespace(), value)),
    )
}

fn normalized_explicit_id(value: u32, generation: ChartGeneration) -> u32 {
    match generation {
        ChartGeneration::Deluxe if value < 10_000 => value.saturating_add(10_000),
        ChartGeneration::Standard if (10_001..100_000).contains(&value) => value - 10_000,
        _ => value,
    }
}

fn chart_constant<T: ToString>(
    value: Option<T>,
    field: &'static str,
) -> Result<Option<ChartConstant>, MusicInfoError> {
    value
        .map(|value| ChartConstant::from_decimal_str(&value.to_string()))
        .transpose()
        .map_err(|_| MusicInfoError::invalid(format!("{field} 超出支持范围")))
}

fn note_detail_score(chart: &SourceChartProjection) -> (usize, bool) {
    let populated = chart.notes.map_or(0, |notes| {
        [
            notes.tap,
            notes.hold,
            notes.slide,
            notes.touch,
            notes.break_notes,
        ]
        .into_iter()
        .filter(|value| *value > 0)
        .count()
    });
    (populated, chart.note_total.is_some())
}

fn difficulties(generation: ChartGeneration) -> &'static [Difficulty] {
    const NORMAL: &[Difficulty] = &[
        Difficulty::Basic,
        Difficulty::Advanced,
        Difficulty::Expert,
        Difficulty::Master,
        Difficulty::ReMaster,
    ];
    const UTAGE: &[Difficulty] = &[Difficulty::Utage];
    if matches!(
        generation,
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
    ) {
        UTAGE
    } else {
        NORMAL
    }
}

fn chart_type(generation: ChartGeneration) -> Option<MusicInfoChartType> {
    match generation {
        ChartGeneration::Standard => Some(MusicInfoChartType::Standard),
        ChartGeneration::Deluxe => Some(MusicInfoChartType::Deluxe),
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer => None,
    }
}

#[cfg(test)]
mod tests {
    use maimai_core::ChartGeneration;

    use super::normalized_explicit_id;

    #[test]
    fn explicit_normal_ids_follow_selected_chart_generation() {
        assert_eq!(normalized_explicit_id(383, ChartGeneration::Deluxe), 10_383);
        assert_eq!(
            normalized_explicit_id(10_383, ChartGeneration::Standard),
            383
        );
        assert_eq!(
            normalized_explicit_id(100_383, ChartGeneration::Deluxe),
            100_383
        );
    }
}
