use maimai_core::{FullComboStatus as ScoreCombo, FullSyncStatus as ScoreSync};
use maimai_render::{CompletionState, FullComboStatus, FullSyncStatus};

use crate::scores::B50Chart;

use super::{CompletionError, CompletionTarget};

pub(crate) fn state(
    record: Option<&B50Chart>,
    target: CompletionTarget,
) -> Result<CompletionState, CompletionError> {
    let achievement = record.and_then(|record| {
        record
            .achievements
            .and_then(|achievement| achievement.ranked())
    });
    let combo = record
        .and_then(|record| record.full_combo)
        .map(combo_status);
    let sync = record.and_then(|record| record.full_sync).map(sync_status);
    Ok(CompletionState {
        completed: target.completed(achievement, combo, sync),
        achievement,
        combo,
        sync,
    })
}

const fn combo_status(value: ScoreCombo) -> FullComboStatus {
    match value {
        ScoreCombo::FullCombo => FullComboStatus::FullCombo,
        ScoreCombo::FullComboPlus => FullComboStatus::FullComboPlus,
        ScoreCombo::AllPerfect => FullComboStatus::AllPerfect,
        ScoreCombo::AllPerfectPlus => FullComboStatus::AllPerfectPlus,
    }
}

const fn sync_status(value: ScoreSync) -> FullSyncStatus {
    match value {
        ScoreSync::Sync | ScoreSync::FullSync => FullSyncStatus::FullSync,
        ScoreSync::FullSyncPlus => FullSyncStatus::FullSyncPlus,
        ScoreSync::FullSyncDeluxe => FullSyncStatus::FullSyncDeluxe,
        ScoreSync::FullSyncDeluxePlus => FullSyncStatus::FullSyncDeluxePlus,
    }
}

#[cfg(test)]
mod tests {
    use super::sync_status;
    use maimai_core::FullSyncStatus as ScoreSync;
    use maimai_render::FullSyncStatus;

    #[test]
    fn fsd_and_fdx_are_one_typed_target_family() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            sync_status(ScoreSync::FullSyncDeluxe),
            FullSyncStatus::FullSyncDeluxe
        );
        assert_eq!(
            sync_status(ScoreSync::FullSyncDeluxePlus),
            FullSyncStatus::FullSyncDeluxePlus
        );
        Ok(())
    }
}
