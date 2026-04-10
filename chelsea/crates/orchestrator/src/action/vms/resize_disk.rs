use thiserror::Error;
use uuid::Uuid;

use crate::action::{Action, AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;
use dto_lib::chelsea_server2::vm::VmResizeDiskRequest;

#[derive(Debug, Clone)]
pub struct ResizeVMDisk {
    key: ApiKeyEntity,
    pub vm_id: Uuid,
    pub fs_size_mib: u32,
    pub skip_wait_boot: bool,
    pub request_id: Option<String>,
}

impl ResizeVMDisk {
    pub fn new(vm_id: Uuid, fs_size_mib: u32, key: ApiKeyEntity, skip_wait_boot: bool) -> Self {
        Self {
            vm_id,
            fs_size_mib,
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
pub enum ResizeVMDiskError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("http error: {0:?}")]
    Http(#[from] HttpError),
    #[error("vm not found")]
    VmNotFound,
    #[error("node not found")]
    NodeNotFound,
    #[error("forbidden")]
    Forbidden,
}

impl Action for ResizeVMDisk {
    type Response = ();
    type Error = ResizeVMDiskError;
    const ACTION_ID: &'static str = "vm.resize_disk";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Check authorization
        let vm = check_vm_access(&ctx.db, &self.key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => ResizeVMDiskError::VmNotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    ResizeVMDiskError::Forbidden
                }
                AuthzError::Db(db) => ResizeVMDiskError::Db(db),
            })?;

        // 2. Get the node where the VM is running
        let vm_node_id = &vm.node_id.ok_or(ResizeVMDiskError::NodeNotFound)?;
        let node = ctx
            .db
            .node()
            .get_by_id(vm_node_id)
            .await?
            .ok_or(ResizeVMDiskError::NodeNotFound)?;

        tracing::info!(
            vm_id = %self.vm_id,
            node_id = %vm_node_id,
            node_ip = %node.ip_priv(),
            fs_size_mib = self.fs_size_mib,
            "Resizing VM disk on Chelsea node"
        );

        // 3. Call Chelsea node to resize the VM disk
        ctx.proto()
            .vm_resize_disk(
                &node,
                self.vm_id,
                VmResizeDiskRequest {
                    fs_size_mib: self.fs_size_mib,
                },
                self.skip_wait_boot,
                self.request_id.as_deref(),
            )
            .await?;

        tracing::info!(
            vm_id = %self.vm_id,
            fs_size_mib = self.fs_size_mib,
            "Successfully resized VM disk"
        );

        Ok(())
    }
}

impl_error_response!(ResizeVMDiskError,
    ResizeVMDiskError::Db(_) => INTERNAL_SERVER_ERROR,
    ResizeVMDiskError::Http(_) => INTERNAL_SERVER_ERROR,
    ResizeVMDiskError::VmNotFound => NOT_FOUND,
    ResizeVMDiskError::NodeNotFound => INTERNAL_SERVER_ERROR,
    ResizeVMDiskError::Forbidden => FORBIDDEN,
);
