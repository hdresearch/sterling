use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;
use dto_lib::chelsea_server2::vm::VmUpdateStateEnum;

#[derive(Debug, Clone)]
pub struct UpdateVMState {
    key: ApiKeyEntity,
    pub vm_id: Uuid,
    pub state: VmUpdateStateEnum,
    pub skip_wait_boot: bool,
    pub request_id: Option<String>,
}

impl UpdateVMState {
    pub fn new(
        vm_id: Uuid,
        state: VmUpdateStateEnum,
        key: ApiKeyEntity,
        skip_wait_boot: bool,
    ) -> Self {
        Self {
            vm_id,
            state,
            key,
            skip_wait_boot,
            request_id: None,
        }
    }

    pub fn pause(vm_id: Uuid, key: ApiKeyEntity, skip_wait_boot: bool) -> Self {
        Self {
            vm_id,
            state: VmUpdateStateEnum::Paused,
            key,
            skip_wait_boot,
            request_id: None,
        }
    }

    pub fn resume(vm_id: Uuid, key: ApiKeyEntity, skip_wait_boot: bool) -> Self {
        Self {
            vm_id,
            state: VmUpdateStateEnum::Running,
            key,
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
pub enum UpdateVMStateError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("http error: {0:?}")]
    Http(#[from] HttpError),
    #[error("vm not found")]
    VmNotFound,
    #[error("node not found")]
    NodeNotFound,

    #[error("The requested VM is not currently running (node_id is null); is it sleeping?")]
    NodeIdNull,

    #[error("forbidden")]
    Forbidden,
}

impl Action for UpdateVMState {
    type Response = ();
    type Error = UpdateVMStateError;
    const ACTION_ID: &'static str = "vm.update_state";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Check authorization
        let vm = check_vm_access(&ctx.db, &self.key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => UpdateVMStateError::VmNotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    UpdateVMStateError::Forbidden
                }
                AuthzError::Db(db) => UpdateVMStateError::Db(db),
            })?;

        // 2. Get the node where the VM is running
        let node_id = vm.node_id.ok_or(UpdateVMStateError::NodeIdNull)?;
        let node = ctx
            .db
            .node()
            .get_by_id(&node_id)
            .await?
            .ok_or(UpdateVMStateError::NodeNotFound)?;

        tracing::info!(
            vm_id = %self.vm_id,
            node_id = %node_id,
            node_ip = %node.ip_priv(),
            state = ?&self.state,
            "Updating VM state on Chelsea node"
        );

        // 3. Call Chelsea node to update VM state
        ctx.proto()
            .vm_update_state(
                &node,
                self.vm_id,
                self.state.clone(),
                self.skip_wait_boot,
                self.request_id.as_deref(),
            )
            .await?;

        // We don't store vm state currently because even chelsea doesn't do that.

        tracing::info!(
            vm_id = %self.vm_id,
            state = ?self.state,
            "Successfully updated VM state"
        );

        Ok(())
    }
}

impl_error_response!(UpdateVMStateError,
    UpdateVMStateError::Db(_) => INTERNAL_SERVER_ERROR,
    UpdateVMStateError::Http(_) => INTERNAL_SERVER_ERROR,
    UpdateVMStateError::VmNotFound => NOT_FOUND,
    UpdateVMStateError::NodeNotFound => INTERNAL_SERVER_ERROR,
    UpdateVMStateError::NodeIdNull => CONFLICT,
    UpdateVMStateError::Forbidden => FORBIDDEN,
);
