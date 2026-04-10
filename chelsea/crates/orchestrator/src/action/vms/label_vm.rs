use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::VMsRepository;
use crate::db::{ApiKeyEntity, DBError};

#[derive(Debug, Clone)]
pub struct LabelVM {
    vm_id: Uuid,
    api_key: ApiKeyEntity,
    labels: Option<HashMap<String, String>>,
    request_id: Option<String>,
}

impl LabelVM {
    pub fn new(
        vm_id: Uuid,
        labels: Option<HashMap<String, String>>,
        api_key: ApiKeyEntity,
    ) -> Self {
        Self {
            vm_id,
            api_key,
            labels,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum LabelVMError {
    #[error("Db error: {0}")]
    Db(#[from] DBError),
    #[error("Internal server error")]
    InternalServerError,
    #[error("vm not found")]
    VmNotFound,
    #[error("Forbidden")]
    Forbidden,
}

impl Action for LabelVM {
    type Response = ();
    type Error = LabelVMError;
    const ACTION_ID: &'static str = "vm.label";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Check authorization
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => LabelVMError::VmNotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    LabelVMError::Forbidden
                }
                AuthzError::Db(db) => LabelVMError::Db(db),
            })?;

        if let Some(labels) = self.labels {
            ctx.db.vms().label(&vm.id(), labels).await?;
            Ok(())
        } else {
            Ok(())
        }
    }
}

impl_error_response!(LabelVMError,
    LabelVMError::Db(_) => INTERNAL_SERVER_ERROR,
    LabelVMError::VmNotFound => NOT_FOUND,
    LabelVMError::InternalServerError => INTERNAL_SERVER_ERROR,
    LabelVMError::Forbidden => FORBIDDEN,
);
