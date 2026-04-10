use thiserror::Error;
use uuid::Uuid;
use vers_config::VersConfig;

use crate::action::{self, Action, ActionContext, ActionError, RechooseNodeError};
use crate::db::{ChelseaNodeRepository, DBError, NodeEntity, VMsRepository};
use crate::outbound::node_proto::HttpError;
use dto_lib::chelsea_server2::vm::{VmWakeRequest, VmWireGuardConfig};

/// Wakes a sleeping VM by restoring it from its sleep snapshot onto a target node.
///
/// A fresh WireGuard port is allocated on the destination node; existing WG private/public keys
/// and IPv6 address are reused so that the VM's identity is preserved. The VM's `node_id` and
/// `wg_port` are updated in the database after a successful wake.
///
/// This action does not require an API key — it is intended for admin/orchestrator use only.
#[derive(Debug, Clone)]
pub struct WakeVM {
    /// The VM to wake. Must currently be sleeping (`node_id IS NULL` with a sleep snapshot).
    pub vm_id: Uuid,
    /// The node to wake the VM on. If `None`, a node is selected automatically via `ChooseNode`.
    pub destination_node_id: Option<Uuid>,
    pub request_id: Option<String>,
}

impl WakeVM {
    pub fn new(vm_id: Uuid, destination_node_id: Option<Uuid>) -> Self {
        Self {
            vm_id,
            destination_node_id,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum WakeVMError {
    #[error("vm not found")]
    VmNotFound,
    /// The VM has a `node_id`, meaning it is already running (not sleeping).
    #[error("vm is not sleeping")]
    VmNotSleeping,
    #[error("destination node not found")]
    NodeNotFound,
    /// No healthy nodes are available (only when `destination_node_id` is `None`).
    #[error("no available nodes: {0}")]
    NoAvailableNodes(#[from] RechooseNodeError),
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("http error: {0:?}")]
    Http(HttpError),
    #[error("internal server error")]
    InternalServerError,
}

impl From<HttpError> for WakeVMError {
    fn from(e: HttpError) -> Self {
        WakeVMError::Http(e)
    }
}

impl Action for WakeVM {
    type Response = Uuid;
    type Error = WakeVMError;
    const ACTION_ID: &'static str = "vm.wake";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Fetch VM record
        let vm = ctx
            .db
            .vms()
            .get_by_id(self.vm_id)
            .await?
            .ok_or(WakeVMError::VmNotFound)?;

        // 2. A present node_id means the VM is already running
        if vm.node_id.is_some() {
            return Err(WakeVMError::VmNotSleeping);
        }

        // 3. Determine destination node
        if let Some(dest_node_id) = self.destination_node_id {
            // Caller specified a node — resolve it and wake there directly
            let node = ctx
                .db
                .node()
                .get_by_id(&dest_node_id)
                .await?
                .ok_or(WakeVMError::NodeNotFound)?;

            let dest_node_id = self.wake_on_node(ctx, &vm, &node).await?;

            Ok(dest_node_id)
        } else {
            // No preference — use ChooseNode to pick the best available node
            let mut candidates = match action::call(action::ChooseNode::new()).await {
                Ok(c) => c,
                Err(ActionError::Error(err)) => return Err(err.into()),
                Err(_) => return Err(WakeVMError::InternalServerError),
            };

            while let Some(reservation) = candidates.next_node() {
                let Some(node) = ctx.db.node().get_by_id(&reservation.node_id()).await? else {
                    continue;
                };

                match self.clone().wake_on_node(ctx, &vm, &node).await {
                    Ok(node_id) => {
                        reservation.commit();
                        return Ok(node_id);
                    }
                    Err(WakeVMError::Http(_)) => {
                        // Node refused; try next candidate
                        tracing::warn!(
                            vm_id = %self.vm_id,
                            node_id = %node.id(),
                            "Failed to wake VM on node, trying next candidate"
                        );
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }

            Err(WakeVMError::InternalServerError)
        }
    }
}

impl WakeVM {
    /// Attempt to wake the VM on a specific node.
    ///
    /// Allocates a WireGuard port on the node, sends the wake request to Chelsea, then
    /// updates `node_id` and `wg_port` in the database.
    async fn wake_on_node(
        self,
        ctx: &ActionContext,
        vm: &crate::db::VmEntity,
        node: &NodeEntity,
    ) -> Result<Uuid, WakeVMError> {
        let node_id = *node.id();

        // Allocate new WG port; new node is not guaranteed to have the old port open
        let wg_port = ctx.db.vms().next_vm_wg_port(node_id).await?;

        let wake_request = VmWakeRequest {
            wireguard: VmWireGuardConfig {
                private_key: vm.wg_private_key.clone(),
                public_key: vm.wg_public_key.clone(),
                ipv6_address: vm.ip.to_string(),
                proxy_ipv6_address: VersConfig::proxy().wg_private_ip.to_string(),
                proxy_public_key: VersConfig::proxy().wg_public_key.to_string(),
                proxy_public_ip: VersConfig::proxy().public_ip.to_string(),
                wg_port,
            },
        };

        tracing::info!(
            vm_id = %self.vm_id,
            node_id = %node_id,
            node_ip = %node.ip_priv(),
            %wg_port,
            "Waking VM on Chelsea node"
        );

        match ctx
            .proto()
            .vm_wake(node, self.vm_id, wake_request, self.request_id.as_deref())
            .await
        {
            Ok(()) => {
                // Success — update DB with the new node and port
                ctx.db.vms().set_node_id(self.vm_id, Some(node_id)).await?;
                ctx.db.vms().set_wg_port(self.vm_id, wg_port).await?;

                tracing::info!(
                    vm_id = %self.vm_id,
                    node_id = %node_id,
                    wg_port = %wg_port,
                    "VM awoken successfully"
                );

                return Ok(node_id);
            }
            Err(error) => {
                tracing::error!(%error, "Error waking VM");
                Err(WakeVMError::InternalServerError)
            }
        }
    }
}
