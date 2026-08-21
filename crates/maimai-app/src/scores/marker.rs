use maimai_core::{FullComboStatus, FullSyncStatus};
use maimai_providers::{FullCombo, FullSync};

pub(super) const fn full_combo(value: FullCombo) -> FullComboStatus {
    match value {
        FullCombo::Fc => FullComboStatus::FullCombo,
        FullCombo::Fcp => FullComboStatus::FullComboPlus,
        FullCombo::Ap => FullComboStatus::AllPerfect,
        FullCombo::App => FullComboStatus::AllPerfectPlus,
    }
}

pub(super) const fn full_sync(value: FullSync) -> FullSyncStatus {
    match value {
        FullSync::Sync => FullSyncStatus::Sync,
        FullSync::Fs => FullSyncStatus::FullSync,
        FullSync::Fsp => FullSyncStatus::FullSyncPlus,
        FullSync::Fsd => FullSyncStatus::FullSyncDeluxe,
        FullSync::Fsdp => FullSyncStatus::FullSyncDeluxePlus,
    }
}
