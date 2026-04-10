use dto_lib::chelsea_server2::vm::{VmExecRequest, VmExecResponse};
use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone)]
pub struct ExecVM {
    pub vm_id: Uuid,
    pub api_key: ApiKeyEntity,
    pub request: VmExecRequest,
    pub request_id: Option<String>,
}

impl ExecVM {
    pub fn new(vm_id: Uuid, api_key: ApiKeyEntity, request: VmExecRequest) -> Self {
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
pub enum ExecVMError {
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

impl Action for ExecVM {
    type Response = VmExecResponse;
    type Error = ExecVMError;
    const ACTION_ID: &'static str = "vm.exec";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => ExecVMError::NotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    ExecVMError::Forbidden
                }
                AuthzError::Db(db) => ExecVMError::Db(db),
            })?;

        let node_id = match vm.node_id {
            Some(node_id) => node_id,
            None => return Err(ExecVMError::InternalServerError),
        };

        let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
            return Err(ExecVMError::InternalServerError);
        };

        let response = ctx
            .proto()
            .vm_exec(&node, vm.id(), self.request, self.request_id.as_deref())
            .await?;

        Ok(response)
    }
}

impl_error_response!(ExecVMError,
    ExecVMError::Db(_) => INTERNAL_SERVER_ERROR,
    ExecVMError::Http(_) => INTERNAL_SERVER_ERROR,
    ExecVMError::NotFound => NOT_FOUND,
    ExecVMError::Forbidden => FORBIDDEN,
    ExecVMError::InternalServerError => INTERNAL_SERVER_ERROR,
);
