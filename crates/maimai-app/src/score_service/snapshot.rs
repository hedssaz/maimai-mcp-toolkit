use maimai_core::{Difficulty, ScoreSource};
use maimai_storage::{PlayerProfile, PlayerRecord};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::scores::{Lookup, PlayerScores};

use super::PlayerScoreServiceErrorCode;
use super::{PlayerScoreService, PlayerScoreServiceError};
use maimai_providers::DivingFishCredentials;

impl PlayerScoreService {
    pub(super) async fn resolve_df_credentials(
        &self,
        explicit: Option<DivingFishCredentials>,
    ) -> Result<DivingFishCredentials, PlayerScoreServiceError> {
        if let Some(credentials) = explicit {
            return Ok(credentials);
        }
        self.store
            .diving_fish_developer_token()
            .await
            .map_err(|_| PlayerScoreServiceError::storage())?
            .map(|token| DivingFishCredentials::new().with_developer_token(token.token().clone()))
            .ok_or_else(|| {
                PlayerScoreServiceError::new(
                    PlayerScoreServiceErrorCode::AuthRequired,
                    "Diving-Fish 完整成绩需要 Developer-Token",
                )
            })
    }

    pub(super) async fn persist_full_scores(
        &self,
        scores: &PlayerScores,
        now: i64,
    ) -> Result<(), PlayerScoreServiceError> {
        let Lookup::Qq(qq) = &scores.lookup else {
            return Ok(());
        };
        let timestamp = OffsetDateTime::from_unix_timestamp(now)
            .map_err(|_| PlayerScoreServiceError::storage())?;
        let updated_at = timestamp
            .format(&Rfc3339)
            .map_err(|_| PlayerScoreServiceError::storage())?;
        let profile = PlayerProfile {
            qq: qq.clone(),
            nickname: scores.player.nickname.clone(),
            player_rating: scores
                .player
                .actual_rating
                .or(scores.player.rating)
                .map(i64::from),
            player_old_rating: None,
            player_new_rating: None,
            score_source: Some(scores.source),
            source_detail: Some(source_detail(scores.source).to_owned()),
            raw: None,
            updated_at: updated_at.clone(),
        };
        let records = scores
            .records
            .iter()
            .map(|chart| PlayerRecord {
                qq: qq.clone(),
                chart: chart.key.clone(),
                title: chart.title.clone(),
                level: Some(chart.level.clone()),
                level_label: Some(difficulty_label(chart.key.difficulty()).to_owned()),
                ds: chart.constant,
                achievements: chart.achievements,
                dx_score: chart.dx_score.map(i64::from),
                fc: chart.full_combo,
                fs: chart.full_sync,
                rate: chart.grade.clone(),
                ra: chart.rating.map(i64::from),
                version: (!chart.version.is_empty()).then(|| chart.version.clone()),
                is_new: chart.is_current,
                score_source: scores.source,
                source_detail: Some(source_detail(scores.source).to_owned()),
                raw: None,
                payload: serde_json::json!({
                    "songId": chart.source_song_id,
                    "title": chart.title,
                    "generation": chart.key.generation(),
                    "difficulty": chart.key.difficulty(),
                    "level": chart.level,
                    "ds": chart.constant,
                    "achievements": chart.achievements,
                    "dxScore": chart.dx_score,
                    "ra": chart.rating,
                    "rate": chart.grade,
                    "fc": chart.full_combo,
                    "fs": chart.full_sync,
                    "version": chart.version,
                }),
                updated_at: updated_at.clone(),
            })
            .collect::<Vec<_>>();
        self.store
            .replace_player_score_snapshot(&profile, &records)
            .await
            .map(|_| ())
            .map_err(|_| PlayerScoreServiceError::storage())
    }
}

fn difficulty_label(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Basic => "Basic",
        Difficulty::Advanced => "Advanced",
        Difficulty::Expert => "Expert",
        Difficulty::Master => "Master",
        Difficulty::ReMaster => "Re:MASTER",
        Difficulty::Utage => "Utage",
    }
}

fn source_detail(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "diving_fish_full_snapshot",
        ScoreSource::Lxns => "lxns_full_snapshot",
        ScoreSource::Local => "local_snapshot",
        ScoreSource::OfficialCn => "official_cn_snapshot",
    }
}
