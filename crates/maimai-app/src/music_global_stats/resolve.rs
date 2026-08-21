use maimai_catalog::{CatalogSnapshot, SearchHit, SourceKind};
use maimai_core::ChartGeneration;
use maimai_render::MusicGlobalStatsView;
use rust_decimal::{Decimal, prelude::ToPrimitive};

use crate::music_info::{MusicInfoChartType, MusicInfoRequest};

use super::{MusicGlobalStatsDifficulty, MusicGlobalStatsError};

pub(super) struct PreparedMusicGlobalStats {
    pub query: String,
    pub music_id: String,
    pub title: String,
    pub chart_type: MusicInfoChartType,
    pub level_index: usize,
    pub view: MusicGlobalStatsView,
}

pub(super) fn prepare(
    snapshot: &CatalogSnapshot,
    request: &MusicInfoRequest,
    hit: &SearchHit<'_>,
    generation: ChartGeneration,
    difficulty: MusicGlobalStatsDifficulty,
) -> Result<PreparedMusicGlobalStats, MusicGlobalStatsError> {
    let chart_type = chart_type(generation)?;
    let prepared =
        crate::music_info::resolve::prepare_catalog_variant(snapshot, request, hit, generation)?;
    if !hit.matched_charts.iter().any(|matched| {
        matched.chart.key.generation() == generation
            && matched.chart.key.difficulty() == difficulty.difficulty()
    }) {
        return Err(MusicGlobalStatsError::invalid(format!(
            "该曲没有 level_index={} 的难度",
            difficulty.index()
        )));
    }
    let stats = hit
        .metadata
        .source_projections
        .iter()
        .filter(|source| source.source == SourceKind::DivingFish)
        .flat_map(|source| &source.charts)
        .find(|chart| chart.generation == generation && chart.difficulty == difficulty.difficulty())
        .and_then(|chart| chart.fit_stats.as_ref())
        .ok_or_else(|| MusicGlobalStatsError::invalid("该曲目/难度没有可用的全服统计数据。"))?;
    let achievement_distribution = distribution::<14>(&stats.distribution, "dist")?;
    let full_combo_distribution = distribution::<5>(&stats.full_combo_distribution, "fc_dist")?;
    let display_id = prepared.music_id.parse::<u32>().ok();
    let view = MusicGlobalStatsView::new(
        display_id,
        prepared.title.clone(),
        difficulty.difficulty(),
        achievement_distribution,
        full_combo_distribution,
    )?;
    Ok(PreparedMusicGlobalStats {
        query: prepared.query,
        music_id: prepared.music_id,
        title: prepared.title,
        chart_type,
        level_index: difficulty.index(),
        view,
    })
}

fn chart_type(value: ChartGeneration) -> Result<MusicInfoChartType, MusicGlobalStatsError> {
    match value {
        ChartGeneration::Standard => Ok(MusicInfoChartType::Standard),
        ChartGeneration::Deluxe => Ok(MusicInfoChartType::Deluxe),
        ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer => Err(
            MusicGlobalStatsError::invalid("全服统计仅支持 standard 或 dx 谱面"),
        ),
    }
}

fn distribution<const N: usize>(
    values: &[Decimal],
    field: &'static str,
) -> Result<[u64; N], MusicGlobalStatsError> {
    if values.len() != N {
        return Err(MusicGlobalStatsError::invalid(format!(
            "全服统计 {field} 必须恰好包含 {N} 项"
        )));
    }
    let converted = values
        .iter()
        .map(|value| count(*value, field))
        .collect::<Result<Vec<_>, _>>()?;
    converted.try_into().map_err(|_| {
        MusicGlobalStatsError::invalid(format!("全服统计 {field} 必须恰好包含 {N} 项"))
    })
}

fn count(value: Decimal, field: &'static str) -> Result<u64, MusicGlobalStatsError> {
    if value.is_sign_negative() || value.trunc() != value {
        return Err(MusicGlobalStatsError::invalid(format!(
            "全服统计 {field} 必须是非负整数"
        )));
    }
    value
        .to_u64()
        .ok_or_else(|| MusicGlobalStatsError::invalid(format!("全服统计 {field} 数值超出范围")))
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::distribution;

    #[test]
    fn distribution_requires_exact_integral_non_negative_counts() {
        assert!(distribution::<14>(&[Decimal::ONE; 13], "dist").is_err());
        assert!(distribution::<5>(&[Decimal::ONE; 6], "fc_dist").is_err());
        let mut fractional = [Decimal::ONE; 14];
        fractional[3] = Decimal::new(15, 1);
        assert!(distribution::<14>(&fractional, "dist").is_err());
        let mut negative = [Decimal::ONE; 5];
        negative[1] = Decimal::NEGATIVE_ONE;
        assert!(distribution::<5>(&negative, "fc_dist").is_err());
    }
}
