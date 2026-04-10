use dto_lib::chelsea_server2::vm::VmWriteFileRequest;
use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone)]
pub struct WriteFileVM {
    pub vm_id: Uuid,
    pub api_key: ApiKeyEntity,
    pub request: VmWriteFileRequest,
    pub request_id: Option<String>,
}

impl WriteFileVM {
    pub fn new(vm_id: Uuid, api_key: ApiKeyEntity, request: VmWriteFileRequest) -> Self {
        Self {
            vm_id,
            api_key,
            request,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum WriteFileVMError {
    #[error("db error: {0}")]
    Db(#[from] DBError),

    #[error("http error: {0}")]
    Http(#[from] HttpError),

    #[error("not found")]
    NotFound,

    #[error("forbidden")]
    Forbidden,

    #[error("internal server error")]
    InternalServerError,
}

impl Action for WriteFileVM {
    type Response = ();
    type Error = WriteFileVMError;
    const ACTION_ID: &'static str = "vm.write_file";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => WriteFileVMError::NotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    WriteFileVMError::Forbidden
                }
                AuthzError::Db(db) => WriteFileVMError::Db(db),
            })?;

        let node_id = match vm.node_id {
            Some(node_id) => node_id,
            None => return Err(WriteFileVMError::InternalServerError),
        };

        let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
            return Err(WriteFileVMError::InternalServerError);
        };

        ctx.proto()
            .vm_write_file(&node, vm.id(), self.request, self.request_id.as_deref())
            .await?;

        Ok(())
    }
}

impl_error_response!(WriteFileVMError,
    WriteFileVMError::Db(_) => INTERNAL_SERVER_ERROR,
    WriteFileVMError::Http(_) => INTERNAL_SERVER_ERROR,
    WriteFileVMError::NotFound => NOT_FOUND,
    WriteFileVMError::Forbidden => FORBIDDEN,
    WriteFileVMError::InternalServerError => INTERNAL_SERVER_ERROR,
);
