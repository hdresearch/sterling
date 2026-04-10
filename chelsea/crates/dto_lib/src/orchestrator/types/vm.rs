use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::chelsea_server2::vm::VmState;

/// Response for GET /api/v1/vm/{vm_id}/metadata
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct VmMetadataResponse {
    pub vm_id: Uuid,
    pub owner_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub state: VmState,
    pub ip: String,
    pub parent_commit_id: Option<Uuid>,
    pub grandparent_vm_id: Option<Uuid>,
}

/// Query parameters for POST /vm/branch/by_vm/{vm_id}
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct BranchVmQuery {
    /// If true, keep the VM paused after commit
    pub keep_paused: Option<bool>,
    /// If true, immediately return an error if VM is booting instead of waiting
    pub skip_wait_boot: Option<bool>,

    // How many VMs? default: 1
    pub count: Option<u8>,
}

/// Query parameters for POST /vm/{vm_id}/branch
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct BranchQuery {
    /// When branching a VM, if true, keep the VM paused after commit
    pub keep_paused: Option<bool>,
    /// When branching a VM, if true, immediately return an error if VM is booting instead of waiting
    pub skip_wait_boot: Option<bool>,

    // How many VMs? default: 1
    pub count: Option<u8>,
}

impl From<BranchQuery> for BranchVmQuery {
    fn from(query: BranchQuery) -> Self {
        BranchVmQuery {
            keep_paused: query.keep_paused,
            skip_wait_boot: query.skip_wait_boot,

            count: query.count,
        }
    }
}
