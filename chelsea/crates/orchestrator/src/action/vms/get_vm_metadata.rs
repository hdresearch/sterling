use dto_lib::orchestrator::vm::VmMetadataResponse;
use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone)]
pub struct GetVMMetadata {
    pub vm_id: Uuid,
    pub api_key: ApiKeyEntity,
    pub request_id: Option<String>,
}

impl GetVMMetadata {
    pub fn by_id(vm_id: Uuid, api_key: ApiKeyEntity) -> Self {
        Self {
            vm_id,
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
pub enum GetVMMetadataError {
    #[error("db error: {0}")]
    Db(#[from] DBError),

    #[error("http error: {0}")]
    Http(#[from] HttpError),

    #[error("not found")]
    NotFound,

    #[error("forbidden")]
    Forbidden,

    #[error("The requested VM is not currently running (node_id is null); is it sleeping?")]
    NodeIdNull,

    #[error("internal server error")]
    InternalServerError,
}

impl Action for GetVMMetadata {
    type Response = VmMetadataResponse;
    type Error = GetVMMetadataError;
    const ACTION_ID: &'static str = "vm.get_metadata";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // Check authorization
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => GetVMMetadataError::NotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    GetVMMetadataError::Forbidden
                }
                AuthzError::Db(db) => GetVMMetadataError::Db(db),
            })?;

        // Fetch the record for the node this VM is on
        let node_id = vm.node_id.ok_or(GetVMMetadataError::NodeIdNull)?;
        let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
            return Err(GetVMMetadataError::InternalServerError);
        };

        // Request the VM status from chelsea
        let state = ctx
            .proto()
            .vm_status(&node, vm.id(), self.request_id.as_deref())
            .await?
            .state;

        // Build the response
        Ok(VmMetadataResponse {
            vm_id: vm.id(),
            owner_id: vm.owner_id(),
            created_at: vm.created_at,
            deleted_at: vm.deleted_at,
            state,
            ip: vm.ip.to_string(),
            parent_commit_id: vm.parent_commit_id,
            grandparent_vm_id: vm.grandparent_vm_id,
        })
    }
}

impl_error_response!(GetVMMetadataError,
    GetVMMetadataError::Db(_) => INTERNAL_SERVER_ERROR,
    GetVMMetadataError::Http(_) => INTERNAL_SERVER_ERROR,
    GetVMMetadataError::NotFound => NOT_FOUND,
    GetVMMetadataError::Forbidden => FORBIDDEN,
    GetVMMetadataError::NodeIdNull => CONFLICT,
    GetVMMetadataError::InternalServerError => INTERNAL_SERVER_ERROR,
);
