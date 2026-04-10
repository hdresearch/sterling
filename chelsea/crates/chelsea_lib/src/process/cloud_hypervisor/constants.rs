use std::path::PathBuf;

use uuid::Uuid;

use crate::process::cloud_hypervisor::config::PathBufJailer;

/// Returns the default log path for a given VM ID: a jailed /cloud-hypervisor.log
pub fn default_log_path(vm_id: Uuid) -> PathBufJailer {
    PathBufJailer::new(vm_id, PathBuf::from("/cloud-hypervisor.log"))
}

/// Returns the default API socket path for a given VM ID: a jailed /run/ch.sock
pub fn default_api_socket_path(vm_id: Uuid) -> PathBufJailer {
    PathBufJailer::new(vm_id, PathBuf::from("/run/ch.sock"))
}
