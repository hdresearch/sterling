mod branch_vm;
mod commit_vm;
mod delete_vm;
mod exec_vm;
mod from_commit_vm;
mod get_vm_exec_logs;
mod get_vm_metadata;
mod get_vm_ssh_key;
mod get_vm_status;
mod label_vm;
mod list_all_vms;
mod list_owner_vms;
mod mark_boot_failed;
mod move_vm;
mod new_root;
mod read_file_vm;
mod reconcile_ghost_vms;
mod resize_disk;
mod sleep_vm;
mod update_state;
mod wake_vm;
mod write_file_vm;

pub use branch_vm::*;
pub use commit_vm::*;
pub use delete_vm::*;
pub use exec_vm::*;
pub use from_commit_vm::*;
pub use get_vm_exec_logs::*;
pub use get_vm_metadata::*;
pub use get_vm_ssh_key::*;
pub use get_vm_status::*;
pub use label_vm::*;
pub use list_all_vms::*;
pub use list_owner_vms::*;
pub use mark_boot_failed::*;
pub use move_vm::*;
pub use new_root::*;
pub use read_file_vm::*;
pub use reconcile_ghost_vms::*;
pub use resize_disk::*;
pub use sleep_vm::*;
pub use update_state::*;
pub use wake_vm::*;
pub use write_file_vm::*;

use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    action::{ActionContext, VmRequirements},
    db::{DB, DBError, EnvVarsRepository, OrganizationEntity, OrgsRepository},
};

/// Error returned when an org exceeds its resource limits.
#[derive(Debug, thiserror::Error)]
#[error("{message}. Increase your limits at https://vers.sh/settings to continue")]
pub struct ResourceLimitError {
    pub message: String,
}

/// Check that creating a VM with `requirements` won't exceed the org's resource limits.
pub async fn check_resource_limits<E>(
    db: &DB,
    org: &OrganizationEntity,
    requirements: &VmRequirements,
) -> Result<(), E>
where
    E: From<DBError> + From<ResourceLimitError>,
{
    let usage = db.orgs().resource_usage(org.id()).await?;

    let new_vcpus = usage.vcpus + requirements.vcpu_count as i64;
    let new_memory = usage.memory_mib + requirements.mem_size_mib as i64;

    if new_vcpus > org.max_vcpus() as i64 {
        return Err(ResourceLimitError {
            message: format!(
                "vCPU limit exceeded: this VM requires {} vCPUs, but org is using {}/{} vCPUs",
                requirements.vcpu_count,
                usage.vcpus,
                org.max_vcpus()
            ),
        }
        .into());
    }

    if new_memory > org.max_memory_mib() {
        return Err(ResourceLimitError {
            message: format!(
                "Memory limit exceeded: this VM requires {} MiB, but org is using {}/{} MiB",
                requirements.mem_size_mib,
                usage.memory_mib,
                org.max_memory_mib()
            ),
        }
        .into());
    }

    Ok(())
}

/// Load all user-defined environment variables for the given API key owner.
/// Returns `None` when the user has not configured any entries.
pub(super) async fn load_user_env_vars(
    ctx: &ActionContext,
    user_id: Uuid,
) -> Result<Option<HashMap<String, String>>, DBError> {
    let vars = ctx.db.env_vars().get_by_user_id(user_id).await?;
    if vars.is_empty() {
        Ok(None)
    } else {
        Ok(Some(vars))
    }
}
