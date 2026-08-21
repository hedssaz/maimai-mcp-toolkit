mod b50;
mod coordination;
mod song;

use std::sync::Arc;

use maimai_core::GroupId;
use maimai_providers::NapCatClient;
use maimai_storage::{RankingJobError, RankingNamespace};
use time::OffsetDateTime;

use super::{RankingError, RankingService, RefreshOptions};

impl RankingService {
    async fn run_background(
        &self,
        namespace: RankingNamespace,
        group_id: GroupId,
        generation: u64,
        client: Arc<NapCatClient>,
        options: RefreshOptions,
        started_at: OffsetDateTime,
    ) -> Result<(), RankingJobError> {
        let result = match namespace {
            RankingNamespace::B50 => {
                self.refresh_b50(&group_id, generation, &client, options, started_at)
                    .await
            }
            RankingNamespace::SongScore => {
                self.refresh_song(&group_id, generation, &client, options, started_at)
                    .await
            }
        };
        if let Err(error) = result {
            let safe = error.safe_job_error();
            match self
                .store
                .fail_ranking_job(namespace, &group_id, generation, self.now(), &safe)
                .await
            {
                Ok(true) => return Ok(()),
                Ok(false) => return Err(safe),
                Err(_) => {
                    let terminal = RankingError::Task.safe_job_error();
                    self.state
                        .lock()
                        .await
                        .terminal_errors
                        .insert((namespace, group_id.as_str().to_owned()), terminal.clone());
                    return Err(terminal);
                }
            }
        }
        Ok(())
    }
}
