use dto_lib::chelsea_server2::vm::VmReadFileResponse;
use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone)]
pub struct ReadFileVM {
    pub vm_id: Uuid,
    pub api_key: ApiKeyEntity,
    pub path: String,
    pub request_id: Option<String>,
}

impl ReadFileVM {
    pub fn new(vm_id: Uuid, api_key: ApiKeyEntity, path: String) -> Self {
        Self {
            vm_id,
            api_key,
            path,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum ReadFileVMError {
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

impl Action for ReadFileVM {
    type Response = VmReadFileResponse;
    type Error = ReadFileVMError;
    const ACTION_ID: &'static str = "vm.read_file";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => ReadFileVMError::NotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    ReadFileVMError::Forbidden
                }
                AuthzError::Db(db) => ReadFileVMError::Db(db),
            })?;

        let node_id = match vm.node_id {
            Some(node_id) => node_id,
            None => return Err(ReadFileVMError::InternalServerError),
        };

        let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
            return Err(ReadFileVMError::InternalServerError);
        };

        let response = ctx
            .proto()
            .vm_read_file(&node, vm.id(), &self.path, self.request_id.as_deref())
            .await?;

        Ok(response)
    }
}

impl_error_response!(ReadFileVMError,
    ReadFileVMError::Db(_) => INTERNAL_SERVER_ERROR,
    ReadFileVMError::Http(_) => INTERNAL_SERVER_ERROR,
    ReadFileVMError::NotFound => NOT_FOUND,
    ReadFileVMError::Forbidden => FORBIDDEN,
    ReadFileVMError::InternalServerError => INTERNAL_SERVER_ERROR,
);
