use maimai_core::ScoreSource;

use crate::{
    score_service::ScoreQuery,
    scores::{SelectionReason, SongFilter, SourceSelection, filter_single_song},
};

use super::{
    MusicIdScores, ScoreBySongError, ScoreBySongRequest, ScoreBySongResult, ScoreBySongService,
};

impl ScoreBySongService {
    pub async fn query(
        &self,
        request: ScoreBySongRequest,
    ) -> Result<ScoreBySongResult, ScoreBySongError> {
        let requested_player = request.player.clone();
        let player = self
            .resolve_player(request.player, request.group_id.as_ref())
            .await?;
        let song = self.resolve_song(&request.song)?;
        let snapshot = self.catalog.snapshot();
        let response = self
            .scores
            .records_with_evidence(
                ScoreQuery {
                    lookup: player.lookup.clone(),
                    source: Some(ScoreSource::DivingFish),
                    diving_fish_credentials: None,
                    now: request.now.unix_timestamp(),
                },
                request.include_raw,
            )
            .await
            .map_err(ScoreBySongError::ScoreQueryFailed)?;
        let (scores, raw, source_selection) = response.into_parts();
        let source_selection = SourceSelection {
            source: ScoreSource::DivingFish,
            reason: SelectionReason::Default,
            ..source_selection
        };
        let records = filter_single_song(
            &scores,
            &SongFilter {
                song: song.canonical_id,
                generation: request.song.generation,
                difficulty: request.song.difficulty,
            },
            &snapshot,
        )
        .map_err(|_| ScoreBySongError::Catalog)?;
        let items = song
            .generation_by_music_id
            .iter()
            .map(|(music_id, generations)| MusicIdScores {
                music_id: *music_id,
                records: records
                    .iter()
                    .filter(|record| generations.contains(&record.key.generation()))
                    .cloned()
                    .collect(),
            })
            .collect();
        Ok(ScoreBySongResult {
            requested_at: request.now,
            requested_player,
            lookup: scores.lookup,
            identity: player.identity,
            song_query: request.song.query,
            selected_song: song.selected,
            selection: song.selection,
            source: scores.source,
            source_selection,
            player: scores.player,
            scores: items,
            raw,
        })
    }
}
