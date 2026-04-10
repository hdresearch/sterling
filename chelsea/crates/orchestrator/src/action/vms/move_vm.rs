use thiserror::Error;
use uuid::Uuid;

use super::{SleepVM, SleepVMError, WakeVM, WakeVMError};
use crate::action::{self, Action, ActionContext, ActionError};

/// Moves a VM from its current node to a destination node.
///
/// This is a composite action equivalent to running [`SleepVM`] followed by [`WakeVM`].
/// The VM is first put to sleep (snapshotted and killed on the current node), then woken
/// on the destination node. If no destination is specified, one is chosen automatically.
///
/// This action does not require an API key — it is intended for admin/orchestrator use only.
#[derive(Debug, Clone)]
pub struct MoveVM {
    /// The VM to move.
    pub vm_id: Uuid,
    /// The destination node. If `None`, a node is selected automatically via `ChooseNode`.
    pub destination_node_id: Option<Uuid>,
    /// If true, error immediately if the VM is still booting rather than waiting.
    pub skip_wait_boot: bool,
    pub request_id: Option<String>,
}

impl MoveVM {
    pub fn new(vm_id: Uuid, destination_node_id: Option<Uuid>, skip_wait_boot: bool) -> Self {
        Self {
            vm_id,
            destination_node_id,
            skip_wait_boot,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum MoveVMError {
    /// The sleep phase failed.
    #[error("sleep failed: {0}")]
    Sleep(#[from] SleepVMError),
    /// The wake phase failed.
    #[error("wake failed: {0}")]
    Wake(#[from] WakeVMError),
    #[error("internal server error")]
    InternalServerError,
}

impl Action for MoveVM {
    /// The `node_id` the VM was woken on.
    type Response = Uuid;
    type Error = MoveVMError;
    const ACTION_ID: &'static str = "vm.move";

    async fn call(self, _ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        // Phase 1: Sleep the VM on its current node
        match action::call(
            SleepVM::new(self.vm_id, self.skip_wait_boot).with_request_id(self.request_id.clone()),
        )
        .await
        {
            Ok(()) => {}
            Err(ActionError::Error(e)) => return Err(MoveVMError::Sleep(e)),
            Err(_) => return Err(MoveVMError::InternalServerError),
        }

        // Phase 2: Wake the VM on the destination node
        let dest_node_id = match action::call(
            WakeVM::new(self.vm_id, self.destination_node_id)
                .with_request_id(self.request_id.clone()),
        )
        .await
        {
            Ok(node_id) => node_id,
            Err(ActionError::Error(e)) => return Err(MoveVMError::Wake(e)),
            Err(_) => return Err(MoveVMError::InternalServerError),
        };

        tracing::info!(
            vm_id = %self.vm_id,
            dest_node_id = %dest_node_id,
            "VM moved successfully"
        );

        Ok(dest_node_id)
    }
}
