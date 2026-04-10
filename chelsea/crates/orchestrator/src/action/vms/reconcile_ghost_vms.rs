use thiserror::Error;
use tracing::{info, warn};
use uuid::Uuid;

use crate::action::Action;
use crate::db::{ChelseaNodeRepository, DBError, VMsRepository};
use crate::outbound::node_proto::HttpError;

/// Reconcile ghost VMs across all Chelsea nodes.
///
/// Iterates all non-deleted VMs in the orchestrator DB, checks their status
/// on the corresponding Chelsea node, and soft-deletes any that Chelsea
/// reports as not found (404).
///
/// This is a safety net that catches ghost VMs from any source:
/// boot failures, partial deletes, crashes, network partitions, etc.
#[derive(Debug, Clone)]
pub struct ReconcileGhostVms {
    pub request_id: Option<String>,
}

impl ReconcileGhostVms {
    pub fn new() -> Self {
        Self { request_id: None }
    }
}

#[derive(Debug, Error)]
pub enum ReconcileGhostVmsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("internal error")]
    InternalError,
}

pub struct ReconcileResult {
    pub ghost_vms_deleted: Vec<Uuid>,
    pub errors: Vec<(Uuid, String)>,
}

impl Action for ReconcileGhostVms {
    type Response = ReconcileResult;
    type Error = ReconcileGhostVmsError;
    const ACTION_ID: &'static str = "vm.reconcile_ghost_vms";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let nodes = ctx.db.node().all_under_orchestrator(ctx.orch.id()).await?;

        let mut ghost_vms_deleted = Vec::new();
        let mut errors = Vec::new();

        for node in &nodes {
            let vms = ctx.db.vms().list_under_node(*node.id()).await?;

            for vm in vms {
                let vm_id = vm.id();

                match ctx
                    .proto()
                    .vm_status(node, vm_id, self.request_id.as_deref())
                    .await
                {
                    Ok(_) => {
                        // VM exists on Chelsea, nothing to do
                    }
                    Err(HttpError::NonSuccessStatusCode(404, _)) => {
                        // Ghost VM: exists in orch DB but not on Chelsea node
                        warn!(%vm_id, node_id = %node.id(), "Reconciliation found ghost VM, marking as deleted");
                        match ctx.db.vms().mark_deleted(&vm_id).await {
                            Ok(()) => ghost_vms_deleted.push(vm_id),
                            Err(db_err) => {
                                warn!(%vm_id, ?db_err, "Failed to soft-delete ghost VM during reconciliation");
                                errors.push((vm_id, db_err.to_string()));
                            }
                        }
                    }
                    Err(e) => {
                        // Transient error (timeout, connection refused, etc.) — skip, don't delete
                        // We only delete on a definitive 404.
                        errors.push((vm_id, e.to_string()));
                    }
                }
            }
        }

        if !ghost_vms_deleted.is_empty() {
            info!(
                count = ghost_vms_deleted.len(),
                "Reconciliation cleaned up ghost VMs"
            );
        }

        Ok(ReconcileResult {
            ghost_vms_deleted,
            errors,
        })
    }
}
