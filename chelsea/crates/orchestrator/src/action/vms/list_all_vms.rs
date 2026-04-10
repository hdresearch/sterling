use chrono::{DateTime, Utc};
use dto_lib::chelsea_server2::vm::VmState;
use futures::{FutureExt, future::join_all};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::action::Action;
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError, VMsRepository};
use crate::outbound::node_proto::HttpError;

/// Check if an error indicates a ghost VM (exists in orch DB but not in chelsea)
fn is_ghost_vm_error(err: &ListAllVMsError) -> bool {
    matches!(
        err,
        ListAllVMsError::Http(HttpError::NonSuccessStatusCode(404, _))
    )
}

/// List all VMs on a specific Chelsea node
///
/// This action is useful for debugging and monitoring. It returns all VMs
/// running on a specific node, including their parent-child relationships.
#[derive(Debug, Clone)]
pub struct ListAllVMs {
    pub api_key: ApiKeyEntity,
    pub request_id: Option<String>,
}

impl ListAllVMs {
    pub fn new(api_key: ApiKeyEntity) -> Self {
        Self {
            api_key,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum ListAllVMsError {
    #[error("http error: {0:?}")]
    DB(#[from] DBError),
    #[error("node not found")]
    NodeNotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("internal server error")]
    InternalServerError,
    #[error("http error: {0:?}")]
    Http(#[from] HttpError),
}

#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VM {
    pub vm_id: Uuid,
    pub owner_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub labels: Option<HashMap<String, String>>,
    pub state: VmState,
}

impl Action for ListAllVMs {
    type Response = Vec<VM>;
    type Error = ListAllVMsError;
    const ACTION_ID: &'static str = "vm.list_all";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let tes = ctx.db.vms().list_by_org_id(self.api_key.org_id()).await?;
        let request_id = self.request_id.clone();

        // For each VM, request its status from chelsea
        // Returns (vm_id, Result) so we can identify which VM failed
        let futs = tes.into_iter().map(|vm| {
            let ctx = ctx;
            let vm_id = vm.id();
            let owner_id = vm.owner_id();
            let created_at = vm.created_at;
            let node_id = vm.node_id;
            let request_id = request_id.clone();
            let labels = vm.labels;

            async move {
                // Skip sleeping VMs (no node assigned)
                let Some(node_id) = node_id else {
                    return Ok(VM {
                        vm_id,
                        owner_id,
                        created_at,
                        labels,
                        state: VmState::Sleeping,
                    });
                };

                // Fetch the record for the node this VM is on
                let node = ctx
                    .db
                    .node()
                    .get_by_id(&node_id)
                    .await?
                    .ok_or(ListAllVMsError::InternalServerError)?;

                // Request the VM's status from chelsea
                let state = ctx
                    .proto()
                    .vm_status(&node, vm_id, request_id.as_deref())
                    .await?
                    .state;

                Ok(VM {
                    vm_id,
                    owner_id,
                    created_at,
                    labels,
                    state,
                })
            }
            .map(move |r| (vm_id, r))
        });

        let results: Vec<(Uuid, Result<VM, ListAllVMsError>)> = join_all(futs).await;

        let mut vms = Vec::new();

        for (vm_id, r) in results {
            match r {
                Ok(vm) => vms.push(vm),
                Err(e) => {
                    if is_ghost_vm_error(&e) {
                        // Ghost VM: exists in orch DB but not in chelsea. Soft-delete it.
                        warn!(%vm_id, "Ghost VM detected, marking as deleted");
                        if let Err(db_err) = ctx.db.vms().mark_deleted(&vm_id).await {
                            warn!(%vm_id, ?db_err, "Failed to soft-delete ghost VM");
                        }
                    } else {
                        warn!(%vm_id, ?e, "Error while getting VM status, skipping");
                    }
                }
            }
        }

        Ok(vms)
    }
}

impl_error_response!(ListAllVMsError,
    ListAllVMsError::DB(_) => INTERNAL_SERVER_ERROR,
    ListAllVMsError::NodeNotFound => INTERNAL_SERVER_ERROR,
    ListAllVMsError::Forbidden => FORBIDDEN,
    ListAllVMsError::InternalServerError => INTERNAL_SERVER_ERROR,
    ListAllVMsError::Http(_) => INTERNAL_SERVER_ERROR,
);
