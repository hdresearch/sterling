use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError, VMsRepository};
use crate::outbound::node_proto::HttpError;

/// Delete a VM from a Chelsea node
#[derive(Debug, Clone)]
pub struct DeleteVM {
    vm_id: Uuid,
    api_key: ApiKeyEntity,
    pub skip_wait_boot: bool,
    pub request_id: Option<String>,
}

impl DeleteVM {
    pub fn new(vm_id: Uuid, api_key: ApiKeyEntity, skip_wait_boot: bool) -> Self {
        Self {
            vm_id,
            api_key,
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
pub enum DeleteVMError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("http-error: {0:?}")]
    Http(#[from] HttpError),
    #[error("Forbidden")]
    Forbidden,
    #[error("node not found")]
    InternalServerError,

    #[error("The requested VM is not currently running (node_id is null); is it sleeping?")]
    NodeIdNull,
}

impl Action for DeleteVM {
    type Response = Uuid;
    type Error = DeleteVMError;
    const ACTION_ID: &'static str = "vm.delete";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Check authorization (returns Forbidden for missing VM to avoid leaking existence)
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => DeleteVMError::Forbidden,
                AuthzError::Forbidden => DeleteVMError::Forbidden,
                AuthzError::Db(db) => DeleteVMError::Db(db),
                AuthzError::CommitNotFound | AuthzError::TagNotFound => DeleteVMError::Forbidden,
            })?;

        // 2. Get the node where the VM is running
        let node_id = vm.node_id.ok_or(DeleteVMError::NodeIdNull)?;
        let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
            return Err(DeleteVMError::InternalServerError);
        };

        // 3. Call Chelsea node to delete the VM (handles recursion on Chelsea side)
        ctx.proto()
            .delete_vm(
                &node,
                &self.vm_id,
                self.skip_wait_boot,
                self.request_id.as_deref(),
            )
            .await?;

        // 4. Mark VM as deleted in DB
        ctx.db.vms().mark_deleted(&self.vm_id).await?;

        tracing::info!(
            vm_id = %self.vm_id,
            "Deleted VM"
        );

        Ok(self.vm_id)
    }
}

impl_error_response!(DeleteVMError,
    DeleteVMError::Db(_) => INTERNAL_SERVER_ERROR,
    DeleteVMError::Http(_) => INTERNAL_SERVER_ERROR,
    DeleteVMError::Forbidden => FORBIDDEN,
    DeleteVMError::InternalServerError => INTERNAL_SERVER_ERROR,
    DeleteVMError::NodeIdNull => CONFLICT,
);
