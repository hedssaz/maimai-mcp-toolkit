use maimai_core::{QqId, ScoreSource};
use maimai_storage::{DivingFishDeveloperToken, StateStore};
use time::OffsetDateTime;

use super::{
    AllowedScoreSources, ClearDeveloperTokenResult, DeveloperToken, DeveloperTokenStatus,
    ScoreSettingsError, ScoreSettingsErrorCode, ScoreSourceSetting,
    model::DEVELOPER_TOKEN_SECURITY_NOTICE,
};

#[derive(Clone, Debug)]
pub struct ScoreSettingsService {
    store: StateStore,
    allowed_sources: AllowedScoreSources,
}

impl ScoreSettingsService {
    pub const fn new(store: StateStore, allowed_sources: AllowedScoreSources) -> Self {
        Self {
            store,
            allowed_sources,
        }
    }

    pub const fn allows(&self, source: ScoreSource) -> bool {
        self.allowed_sources.allows(source)
    }

    pub async fn bind_developer_token(
        &self,
        token: DeveloperToken,
        updated_at: OffsetDateTime,
    ) -> Result<DeveloperTokenStatus, ScoreSettingsError> {
        let metadata = self
            .store
            .set_diving_fish_developer_token(token.secret(), updated_at)
            .await
            .map_err(ScoreSettingsError::TokenStorage)?;
        Ok(DeveloperTokenStatus {
            bound: true,
            updated_at: Some(metadata.updated_at),
            security_notice: DEVELOPER_TOKEN_SECURITY_NOTICE,
        })
    }

    pub async fn developer_token_status(&self) -> Result<DeveloperTokenStatus, ScoreSettingsError> {
        let metadata = self
            .store
            .diving_fish_developer_token_metadata()
            .await
            .map_err(ScoreSettingsError::TokenStorage)?;
        Ok(DeveloperTokenStatus {
            bound: metadata.is_some(),
            updated_at: metadata.map(|value| value.updated_at),
            security_notice: DEVELOPER_TOKEN_SECURITY_NOTICE,
        })
    }

    pub async fn developer_token(
        &self,
    ) -> Result<Option<DivingFishDeveloperToken>, ScoreSettingsError> {
        self.store
            .diving_fish_developer_token()
            .await
            .map_err(ScoreSettingsError::TokenStorage)
    }

    pub async fn clear_developer_token(
        &self,
    ) -> Result<ClearDeveloperTokenResult, ScoreSettingsError> {
        let cleared = self
            .store
            .clear_diving_fish_developer_token()
            .await
            .map_err(ScoreSettingsError::TokenStorage)?;
        Ok(ClearDeveloperTokenResult {
            bound: false,
            cleared,
        })
    }

    pub async fn switch_score_source(
        &self,
        qq: QqId,
        source: ScoreSource,
    ) -> Result<ScoreSourceSetting, ScoreSettingsError> {
        if !self.allowed_sources.allows(source) {
            return Err(ScoreSettingsError::public(
                ScoreSettingsErrorCode::SourceNotAllowed,
                "当前部署不支持该成绩数据源。",
            ));
        }
        self.store
            .set_score_source_preference(&qq, source)
            .await
            .map_err(ScoreSettingsError::SourcePreferenceStorage)?;
        Ok(ScoreSourceSetting { qq, source })
    }
}
