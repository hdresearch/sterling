use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, ActionContext};
use crate::db::{ChelseaNodeRepository, DBError, VMsRepository};
use crate::outbound::node_proto::HttpError;

/// Sleeps a VM by snapshotting it and killing its process on the current node.
///
/// After this action succeeds, Chelsea has created a `chelsea.sleep_snapshot` for the VM and
/// the VM's `node_id` is set to `NULL` in the database. The VM is not deleted; it can be
/// resumed with [`WakeVM`].
///
/// This action does not require an API key — it is intended for admin/orchestrator use only.
#[derive(Debug, Clone)]
pub struct SleepVM {
    /// The VM to put to sleep.
    pub vm_id: Uuid,
    /// If true, error immediately rather than waiting for a booting VM to finish.
    pub skip_wait_boot: bool,
    pub request_id: Option<String>,
}

impl SleepVM {
    pub fn new(vm_id: Uuid, skip_wait_boot: bool) -> Self {
        Self {
            vm_id,
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
pub enum SleepVMError {
    #[error("vm not found")]
    VmNotFound,
    /// The VM has no `node_id`, meaning it is already sleeping (or in an invalid state).
    #[error("vm is already sleeping")]
    VmAlreadySleeping,
    #[error("node not found")]
    NodeNotFound,
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("http error: {0:?}")]
    Http(#[from] HttpError),
}

impl Action for SleepVM {
    type Response = ();
    type Error = SleepVMError;
    const ACTION_ID: &'static str = "vm.sleep";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Fetch VM record
        let vm = ctx
            .db
            .vms()
            .get_by_id(self.vm_id)
            .await?
            .ok_or(SleepVMError::VmNotFound)?;

        // 2. A missing node_id means the VM is already sleeping (or invalid state)
        let node_id = vm.node_id.ok_or(SleepVMError::VmAlreadySleeping)?;

        // 3. Resolve node
        let node = ctx
            .db
            .node()
            .get_by_id(&node_id)
            .await?
            .ok_or(SleepVMError::NodeNotFound)?;

        tracing::info!(
            vm_id = %self.vm_id,
            node_id = %node_id,
            node_ip = %node.ip_priv(),
            "Sleeping VM on Chelsea node"
        );

        // 4. Tell Chelsea to snapshot and kill the VM process
        ctx.proto()
            .vm_sleep(
                &node,
                self.vm_id,
                self.skip_wait_boot,
                self.request_id.as_deref(),
            )
            .await?;

        // 5. Clear the node_id in the DB — the VM is now sleeping
        ctx.db.vms().set_node_id(self.vm_id, None).await?;

        tracing::info!(vm_id = %self.vm_id, "VM is now sleeping");

        Ok(())
    }
}
