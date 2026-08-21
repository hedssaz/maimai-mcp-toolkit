use maimai_storage::{IdentityJobError, IdentityMetadata, IdentityStats};
use time::OffsetDateTime;
use tokio::{sync::watch, task::JoinHandle};

use super::super::IdentityError;

#[derive(Clone, Debug)]
pub(crate) enum RefreshOutcome {
    Succeeded(IdentityMetadata),
    Failed(IdentityJobError),
}

#[derive(Clone, Debug)]
pub(super) enum PendingTerminalWrite {
    Complete {
        generation: u64,
        finished_at: OffsetDateTime,
        stats: IdentityStats,
    },
    Fail {
        generation: u64,
        finished_at: OffsetDateTime,
        error: IdentityJobError,
    },
}

#[derive(Debug, Default)]
pub(crate) struct RefreshState {
    pub(super) recovered: bool,
    pub(super) next_flight_id: u64,
    pub(super) active: Option<ActiveRefresh>,
    pub(super) pending_terminal: Option<PendingTerminalWrite>,
}

#[derive(Debug)]
pub(super) struct ActiveRefresh {
    pub(super) id: u64,
    pub(super) generation: Option<u64>,
    pub(super) done: watch::Receiver<Option<RefreshOutcome>>,
    pub(super) task: Option<JoinHandle<Result<(), IdentityJobError>>>,
}

pub(super) struct StartedFlight {
    pub(super) id: u64,
    pub(super) sender: watch::Sender<Option<RefreshOutcome>>,
    pub(super) receiver: watch::Receiver<Option<RefreshOutcome>>,
}

pub(crate) enum RefreshFlight {
    Leader {
        id: u64,
        receiver: watch::Receiver<Option<RefreshOutcome>>,
    },
    Follower(watch::Receiver<Option<RefreshOutcome>>),
}

impl RefreshState {
    pub(super) fn start_flight(
        &mut self,
        generation: Option<u64>,
    ) -> Result<StartedFlight, IdentityError> {
        self.next_flight_id = self
            .next_flight_id
            .checked_add(1)
            .ok_or(IdentityError::RefreshGenerationOverflow)?;
        let (sender, receiver) = watch::channel(None);
        self.active = Some(ActiveRefresh {
            id: self.next_flight_id,
            generation,
            done: receiver.clone(),
            task: None,
        });
        Ok(StartedFlight {
            id: self.next_flight_id,
            sender,
            receiver,
        })
    }

    pub(super) fn attach_task(
        &mut self,
        flight_id: u64,
        task: JoinHandle<Result<(), IdentityJobError>>,
    ) {
        if let Some(active) = self.active.as_mut().filter(|active| active.id == flight_id) {
            active.task = Some(task);
        }
    }
}
