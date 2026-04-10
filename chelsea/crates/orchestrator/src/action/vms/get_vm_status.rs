use dto_lib::chelsea_server2::vm::VmState;
use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, VM, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone)]
pub struct GetVMStatus {
    pub vm_id: Uuid,
    pub api_key: ApiKeyEntity,
    pub request_id: Option<String>,
}
impl GetVMStatus {
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
pub enum GetVMStatusError {
    #[error("db error: {0}")]
    Db(#[from] DBError),

    #[error("db error: {0}")]
    Http(#[from] HttpError),
    #[error("not found")]
    NotFound,

    #[error("forbidden")]
    Forbidden,

    #[error("internal server error")]
    InternalServerError,

    #[error("The requested VM is not currently running (node_id is null); is it sleeping?")]
    NodeIdNull,
}

impl Action for GetVMStatus {
    type Response = VM;
    type Error = GetVMStatusError;
    const ACTION_ID: &'static str = "vm.get";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // Check authorization
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => GetVMStatusError::NotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    GetVMStatusError::Forbidden
                }
                AuthzError::Db(db) => GetVMStatusError::Db(db),
            })?;

        // Fetch the record for the node this VM is on
        let node_id = vm.node_id.ok_or(GetVMStatusError::NodeIdNull)?;
        let node = ctx.db.node().get_by_id(&node_id).await?;

        // Request the VM status from chelsea; if node_id is None, assume sleeping
        let state = match node {
            Some(node) => {
                ctx.proto()
                    .vm_status(&node, vm.id(), self.request_id.as_deref())
                    .await?
                    .state
            }
            None => VmState::Sleeping,
        };

        Ok(VM {
            vm_id: vm.id(),
            owner_id: vm.owner_id(),
            created_at: vm.created_at,
            labels: vm.labels,
            state,
        })
    }
}

impl_error_response!(GetVMStatusError,
    GetVMStatusError::Db(_) => INTERNAL_SERVER_ERROR,
    GetVMStatusError::Http(_) => INTERNAL_SERVER_ERROR,
    GetVMStatusError::NotFound => NOT_FOUND,
    GetVMStatusError::Forbidden => FORBIDDEN,
    GetVMStatusError::InternalServerError => INTERNAL_SERVER_ERROR,
    GetVMStatusError::NodeIdNull => CONFLICT,
);
