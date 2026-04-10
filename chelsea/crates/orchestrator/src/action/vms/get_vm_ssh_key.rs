use dto_lib::chelsea_server2::vm::VmSshKeyResponse;
use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone)]
pub struct GetVMSshKey {
    pub vm_id: Uuid,
    pub api_key: ApiKeyEntity,
    pub request_id: Option<String>,
}
impl GetVMSshKey {
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
pub enum GetVMSshKeyError {
    #[error("db error: {0}")]
    Db(#[from] DBError),

    #[error("db error: {0}")]
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

impl Action for GetVMSshKey {
    type Response = VmSshKeyResponse;
    type Error = GetVMSshKeyError;
    const ACTION_ID: &'static str = "vm.get_ssh_key";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // Check authorization
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => GetVMSshKeyError::NotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    GetVMSshKeyError::Forbidden
                }
                AuthzError::Db(db) => GetVMSshKeyError::Db(db),
            })?;

        // Fetch the record for the node this VM is on.
        let node_id = vm.node_id.ok_or(GetVMSshKeyError::NodeIdNull)?;
        let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
            return Err(GetVMSshKeyError::InternalServerError);
        };

        // Request the SSH key from chelsea
        let ssh = ctx
            .proto()
            .ssh_key(&node, vm.id(), self.request_id.as_deref())
            .await?;

        Ok(ssh)
    }
}

impl_error_response!(GetVMSshKeyError,
    GetVMSshKeyError::Db(_) => INTERNAL_SERVER_ERROR,
    GetVMSshKeyError::Http(_) => INTERNAL_SERVER_ERROR,
    GetVMSshKeyError::NotFound => NOT_FOUND,
    GetVMSshKeyError::Forbidden => FORBIDDEN,
    GetVMSshKeyError::NodeIdNull => CONFLICT,
    GetVMSshKeyError::InternalServerError => INTERNAL_SERVER_ERROR,
);
