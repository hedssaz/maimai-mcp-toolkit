use maimai_catalog::{CatalogQuery, CatalogSnapshot, SourceKind};
use maimai_core::{
    ChartConstant, ChartGeneration, ChartKey, Difficulty, SongIdNamespace, SongIdValue,
    SourceSongId,
};

use super::{ScoreError, ScoreErrorCode};

#[derive(Clone, Copy)]
pub(super) enum GenerationMatch {
    Exact(ChartGeneration),
    Utage,
}

#[derive(Clone)]
pub(super) struct ResolvedChart {
    pub key: ChartKey,
    pub title: String,
    pub level: String,
    pub constant: Option<ChartConstant>,
    pub fit_constant: Option<ChartConstant>,
    pub version: String,
    pub is_current: bool,
}

struct CatalogChart {
    song_ids: Vec<SourceSongId>,
    chart_ids: Vec<SourceSongId>,
    source_versions: Vec<(SourceSongId, String)>,
    resolved: ResolvedChart,
}

pub(super) struct ScoreCatalog {
    charts: Vec<CatalogChart>,
}

impl ScoreCatalog {
    pub fn new(snapshot: &CatalogSnapshot) -> Result<Self, ScoreError> {
        let current_versions = snapshot.current_diving_fish_versions();
        let hits = snapshot
            .query(&CatalogQuery::default())
            .map_err(|error| ScoreError::catalog(error.to_string()))?;
        let mut charts = Vec::new();
        for hit in hits {
            for matched in hit.matched_charts {
                let current_projection = hit.metadata.source_projections.iter().find(|source| {
                    source.source == SourceKind::DivingFish
                        && current_versions.contains(&source.version)
                        && source.charts.iter().any(|chart| {
                            chart.generation == matched.chart.key.generation()
                                && chart.difficulty == matched.chart.key.difficulty()
                        })
                });
                let fit_constant = matched
                    .metadata
                    .fit_diff
                    .map(|value| ChartConstant::from_decimal_str(&value.to_string()))
                    .transpose()
                    .map_err(|_| ScoreError::catalog("catalog fit_diff 超出定数范围"))?;
                let mut song_ids = hit.music.source_ids.clone();
                let mut chart_ids = matched.chart.source_ids.clone();
                let mut source_versions = Vec::new();
                for source in &hit.metadata.source_projections {
                    push_unique(&mut song_ids, source.id.clone());
                    for chart in &source.charts {
                        if chart.generation == matched.chart.key.generation()
                            && chart.difficulty == matched.chart.key.difficulty()
                        {
                            push_unique(&mut chart_ids, chart.source_song_id.clone());
                            push_unique(
                                &mut source_versions,
                                (source.id.clone(), source.version.clone()),
                            );
                        }
                    }
                }
                charts.push(CatalogChart {
                    song_ids,
                    chart_ids,
                    source_versions,
                    resolved: ResolvedChart {
                        key: matched.chart.key.clone(),
                        title: hit.music.title.clone(),
                        level: matched.chart.level.clone(),
                        constant: matched.chart.constant,
                        fit_constant,
                        version: hit.music.version.clone(),
                        is_current: current_projection.is_some(),
                    },
                });
            }
        }
        Ok(Self { charts })
    }

    pub fn resolve_chart(
        &self,
        source_id: &SourceSongId,
        generation: GenerationMatch,
        difficulty: Difficulty,
    ) -> Result<ResolvedChart, ScoreError> {
        let aliases = source_aliases(source_id, generation);
        let mut matches = self
            .charts
            .iter()
            .filter(|entry| generation.matches(entry.resolved.key.generation()))
            .filter(|entry| entry.resolved.key.difficulty() == difficulty)
            .filter(|entry| {
                aliases.iter().any(|candidate| {
                    entry.chart_ids.contains(candidate) || entry.song_ids.contains(candidate)
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.resolved.key.cmp(&right.resolved.key));
        matches.dedup_by(|left, right| left.resolved.key == right.resolved.key);
        match matches.as_slice() {
            [only] => {
                let mut resolved = only.resolved.clone();
                if let Some(version) = source_version(only, &aliases, source_id.namespace()) {
                    resolved.version = version.to_owned();
                }
                Ok(resolved)
            }
            [] => Err(ScoreError::new(
                ScoreErrorCode::ChartNotFound,
                format!("catalog 中没有匹配的谱面：{source_id:?}"),
            )),
            _ => Err(ScoreError::new(
                ScoreErrorCode::AmbiguousChart,
                format!("来源 ID 对应多个谱面：{source_id:?}"),
            )),
        }
    }

    pub fn canonical_song(&self, source_id: &SourceSongId) -> Result<SourceSongId, ScoreError> {
        let mut matches = self
            .charts
            .iter()
            .filter(|entry| {
                entry.song_ids.contains(source_id) || entry.chart_ids.contains(source_id)
            })
            .map(|entry| entry.resolved.key.song().clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        match matches.as_slice() {
            [only] => Ok(only.clone()),
            [] => Err(ScoreError::new(
                ScoreErrorCode::ChartNotFound,
                format!("catalog 中没有来源曲目：{source_id:?}"),
            )),
            _ => Err(ScoreError::new(
                ScoreErrorCode::AmbiguousChart,
                format!("来源 ID 对应多个曲目：{source_id:?}"),
            )),
        }
    }
}

fn source_version<'a>(
    entry: &'a CatalogChart,
    aliases: &[SourceSongId],
    namespace: SongIdNamespace,
) -> Option<&'a str> {
    for candidate in aliases {
        if let Some((_, version)) = entry
            .source_versions
            .iter()
            .find(|(source_id, _)| source_id == candidate)
        {
            return Some(version);
        }
    }
    let mut versions = entry
        .source_versions
        .iter()
        .filter(|(source_id, _)| source_id.namespace() == namespace)
        .map(|(_, version)| version.as_str())
        .collect::<Vec<_>>();
    versions.sort_unstable();
    versions.dedup();
    match versions.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

impl GenerationMatch {
    fn matches(self, generation: ChartGeneration) -> bool {
        match self {
            Self::Exact(expected) => generation == expected,
            Self::Utage => matches!(
                generation,
                ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
            ),
        }
    }
}

fn source_aliases(source_id: &SourceSongId, generation: GenerationMatch) -> Vec<SourceSongId> {
    let mut values = vec![source_id.clone()];
    if source_id.namespace() == SongIdNamespace::DivingFish
        && matches!(generation, GenerationMatch::Exact(ChartGeneration::Deluxe))
        && let SongIdValue::Numeric(value) = source_id.value()
        && *value < 10_000
        && let Some(value) = value.checked_add(10_000)
    {
        values.push(SourceSongId::numeric(SongIdNamespace::DivingFish, value));
    }
    values
}

fn push_unique<T: Eq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}
