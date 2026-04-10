use dto_lib::chelsea_server2::vm::{VmExecLogQuery, VmExecLogResponse};
use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone)]
pub struct GetVMExecLogs {
    pub vm_id: Uuid,
    pub api_key: ApiKeyEntity,
    pub query: VmExecLogQuery,
    pub request_id: Option<String>,
}

impl GetVMExecLogs {
    pub fn new(vm_id: Uuid, api_key: ApiKeyEntity, query: VmExecLogQuery) -> Self {
        Self {
            vm_id,
            api_key,
            query,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum GetVMExecLogsError {
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

impl Action for GetVMExecLogs {
    type Response = VmExecLogResponse;
    type Error = GetVMExecLogsError;
    const ACTION_ID: &'static str = "vm.exec_logs";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => GetVMExecLogsError::NotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    GetVMExecLogsError::Forbidden
                }
                AuthzError::Db(db) => GetVMExecLogsError::Db(db),
            })?;

        let node_id = match vm.node_id {
            Some(node_id) => node_id,
            None => return Err(GetVMExecLogsError::InternalServerError),
        };

        let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
            return Err(GetVMExecLogsError::InternalServerError);
        };

        let response = ctx
            .proto()
            .vm_exec_logs(&node, vm.id(), &self.query, self.request_id.as_deref())
            .await?;

        Ok(response)
    }
}

impl_error_response!(GetVMExecLogsError,
    GetVMExecLogsError::Db(_) => INTERNAL_SERVER_ERROR,
    GetVMExecLogsError::Http(_) => INTERNAL_SERVER_ERROR,
    GetVMExecLogsError::NotFound => NOT_FOUND,
    GetVMExecLogsError::Forbidden => FORBIDDEN,
    GetVMExecLogsError::InternalServerError => INTERNAL_SERVER_ERROR,
);
