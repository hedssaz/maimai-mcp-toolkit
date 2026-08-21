mod filters;
mod sort;
mod source_filter;
mod text;
mod types;

pub use types::{
    CatalogQuery, CatalogSort, FitLabel, InclusiveRange, MatchKind, MatchedChart, NewSongSource,
    QueryError, SearchHit, SongIdFilter, SortDirection, SortKey, SourceChartMatch,
};

use crate::CatalogSnapshot;
use filters::{chart_matches, music_matches, validate};
use sort::sort_hits;
use source_filter::source_chart_matches;
use text::match_kind;

impl CatalogSnapshot {
    pub fn query<'a>(&'a self, query: &CatalogQuery) -> Result<Vec<SearchHit<'a>>, QueryError> {
        validate(query)?;
        Ok(execute_supported(self, query))
    }

    pub fn resolve_tag(&self, value: &str) -> Result<u32, QueryError> {
        let normalized = self.normalizer().normalize(value);
        self.metadata()
            .tag_names
            .get(&normalized)
            .copied()
            .ok_or_else(|| QueryError::UnknownTag(value.to_owned()))
    }

    pub fn tag_label(&self, tag_id: u32) -> Option<&str> {
        self.metadata().tag_labels.get(&tag_id).map(String::as_str)
    }

    pub fn version_order(&self) -> &[String] {
        &self.metadata().version_order
    }

    pub fn latest_cn_versions(&self) -> &[u32] {
        &self.metadata().latest_cn_versions
    }

    pub fn current_diving_fish_versions(&self) -> &[String] {
        &self.metadata().current_diving_fish_versions
    }

    pub fn song_metadata(&self, index: usize) -> Option<&crate::SongQueryMetadata> {
        self.metadata().songs.get(index)
    }

    pub fn to_simplified(&self, value: &str) -> String {
        value
            .chars()
            .map(|character| {
                self.metadata()
                    .traditional_to_simplified
                    .get(&character)
                    .copied()
                    .unwrap_or(character)
            })
            .collect()
    }
}

pub(crate) fn execute_supported<'a>(
    snapshot: &'a CatalogSnapshot,
    query: &CatalogQuery,
) -> Vec<SearchHit<'a>> {
    let mut hits = snapshot
        .songs()
        .iter()
        .enumerate()
        .filter_map(|(song_index, music)| {
            let (matched_by, matched_value) =
                match_kind(snapshot, song_index, query.query.as_deref())?;
            let song_metadata = snapshot.metadata().songs.get(song_index)?;
            if !music_matches(snapshot, music, song_metadata, query) {
                return None;
            }
            let matched_charts = music
                .charts
                .iter()
                .enumerate()
                .filter_map(|(chart_index, chart)| {
                    let metadata = snapshot
                        .metadata()
                        .charts
                        .get(song_index)?
                        .get(chart_index)?;
                    let source_matches = song_metadata
                        .source_projections
                        .iter()
                        .flat_map(|source| {
                            source
                                .charts
                                .iter()
                                .filter(move |source_chart| {
                                    source_chart.generation == chart.key.generation()
                                        && source_chart.difficulty == chart.key.difficulty()
                                })
                                .filter(|source_chart| {
                                    source_chart_matches(
                                        snapshot,
                                        source,
                                        source_chart,
                                        metadata,
                                        query,
                                    )
                                })
                                .map(move |source_chart| SourceChartMatch {
                                    song: source,
                                    chart: source_chart,
                                })
                        })
                        .collect::<Vec<_>>();
                    let matched = if song_metadata.source_projections.is_empty() {
                        chart_matches(snapshot, music, chart, song_metadata, metadata, query)
                    } else {
                        !source_matches.is_empty()
                    };
                    matched.then(|| MatchedChart::new(chart, metadata, chart_index, source_matches))
                })
                .collect::<Vec<_>>();
            if matched_charts.is_empty() {
                return None;
            }
            Some(SearchHit {
                music,
                metadata: song_metadata,
                matched_by,
                matched_value,
                matched_charts,
                song_index,
            })
        })
        .collect::<Vec<_>>();

    let has_nonempty_text = query
        .query
        .as_deref()
        .is_some_and(|value| !snapshot.normalizer().normalize(value).is_empty());
    if query.sort.key != SortKey::Relevance || has_nonempty_text {
        sort_hits(snapshot, &mut hits, query.sort);
    }
    if let Some(limit) = query.limit {
        hits.truncate(limit);
    }
    hits
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, error::Error};

    use maimai_core::{
        Chart, ChartConstant, ChartGeneration, ChartKey, Difficulty, Music, NoteCounts,
        SongIdNamespace, SongIdValue, SourceSongId,
    };

    use super::{
        CatalogQuery, CatalogSort, InclusiveRange, MatchKind, SongIdFilter, SortDirection, SortKey,
    };
    use crate::CatalogSnapshot;

    #[test]
    fn combines_song_and_chart_filters_across_standard_and_deluxe() -> Result<(), Box<dyn Error>> {
        let snapshot = fixture()?;
        let query = CatalogQuery {
            id: Some(SongIdFilter::AnySource(SongIdValue::Numeric(10_010))),
            genre: Some("game".to_owned()),
            version: Some("prism".to_owned()),
            level: Some("13+".to_owned()),
            constant: InclusiveRange::new(
                Some(ChartConstant::from_hundredths(1_350)?),
                Some(ChartConstant::from_hundredths(1_400)?),
            ),
            difficulties: BTreeSet::from([Difficulty::Master]),
            generations: BTreeSet::from([ChartGeneration::Deluxe]),
            bpm: InclusiveRange::new(Some(150), Some(170)),
            artist: Some("ali".to_owned()),
            charter: Some("car".to_owned()),
            ..CatalogQuery::default()
        };
        let hits = snapshot.query(&query)?;

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].music.title, "Alpha");
        assert_eq!(hits[0].matched_by, MatchKind::FilterOnly);
        assert_eq!(hits[0].matched_charts.len(), 1);
        assert_eq!(
            hits[0].matched_charts[0].chart.key.generation(),
            ChartGeneration::Deluxe
        );
        assert_eq!(
            hits[0].matched_charts[0].constant_display.as_deref(),
            Some("13.7")
        );
        Ok(())
    }

    #[test]
    fn keeps_integer_constant_display_at_one_decimal_and_filters_standard()
    -> Result<(), Box<dyn Error>> {
        let snapshot = fixture()?;
        let query = CatalogQuery {
            query: Some("alpha".to_owned()),
            generations: BTreeSet::from([ChartGeneration::Standard]),
            ..CatalogQuery::default()
        };
        let hits = snapshot.query(&query)?;

        assert_eq!(hits[0].matched_by, MatchKind::ExactTitle);
        assert_eq!(hits[0].matched_charts.len(), 1);
        assert_eq!(
            hits[0].matched_charts[0].constant_display.as_deref(),
            Some("12.0")
        );
        Ok(())
    }

    #[test]
    fn text_ranking_sort_and_limit_are_stable() -> Result<(), Box<dyn Error>> {
        let snapshot = fixture()?;
        let ranked = snapshot.search_text("alpha", 10);
        let kinds = ranked.iter().map(|hit| hit.matched_by).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                MatchKind::ExactTitle,
                MatchKind::ExactAlias,
                MatchKind::TitlePrefix,
                MatchKind::TitleContains,
            ]
        );

        let sorted = snapshot.query(&CatalogQuery {
            genre: Some("game".to_owned()),
            sort: CatalogSort {
                key: SortKey::Title,
                direction: SortDirection::Descending,
            },
            limit: Some(1),
            ..CatalogQuery::default()
        })?;
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].music.title, "Xalpha");
        Ok(())
    }

    fn fixture() -> Result<CatalogSnapshot, Box<dyn Error>> {
        let alpha_id = SourceSongId::numeric(SongIdNamespace::Lxns, 10);
        let songs = vec![
            Music {
                primary_id: alpha_id.clone(),
                source_ids: vec![
                    alpha_id.clone(),
                    SourceSongId::numeric(SongIdNamespace::DivingFish, 10_010),
                ],
                title: "Alpha".to_owned(),
                artist: "Alice".to_owned(),
                genre: "Game & Variety".to_owned(),
                version: "PRiSM".to_owned(),
                bpm: 160,
                aliases: vec!["Beginning".to_owned()],
                charts: vec![
                    chart(
                        alpha_id.clone(),
                        ChartGeneration::Standard,
                        Difficulty::Master,
                        "12",
                        1_200,
                        "Bob",
                    )?,
                    chart(
                        alpha_id,
                        ChartGeneration::Deluxe,
                        Difficulty::Master,
                        "13+",
                        1_370,
                        "Carol",
                    )?,
                ],
            },
            song(11, "Hero", &["Alpha"], 150)?,
            song(12, "Alpha Centauri", &[], 140)?,
            song(13, "Xalpha", &[], 130)?,
        ];
        Ok(CatalogSnapshot::from_test_songs(songs)?)
    }

    fn song(id: u32, title: &str, aliases: &[&str], bpm: u32) -> Result<Music, Box<dyn Error>> {
        let source_id = SourceSongId::numeric(SongIdNamespace::Lxns, id);
        Ok(Music {
            primary_id: source_id.clone(),
            source_ids: vec![source_id.clone()],
            title: title.to_owned(),
            artist: "Alice".to_owned(),
            genre: "Game & Variety".to_owned(),
            version: "PRiSM".to_owned(),
            bpm,
            aliases: aliases.iter().map(|value| (*value).to_owned()).collect(),
            charts: vec![chart(
                source_id,
                ChartGeneration::Standard,
                Difficulty::Master,
                "12",
                1_200,
                "Bob",
            )?],
        })
    }

    fn chart(
        song: SourceSongId,
        generation: ChartGeneration,
        difficulty: Difficulty,
        level: &str,
        hundredths: u32,
        charter: &str,
    ) -> Result<Chart, Box<dyn Error>> {
        Ok(Chart {
            key: ChartKey::new(song.clone(), generation, difficulty)?,
            source_ids: vec![song],
            level: level.to_owned(),
            constant: Some(ChartConstant::from_hundredths(hundredths)?),
            note_designer: charter.to_owned(),
            notes: NoteCounts::default(),
        })
    }
}
