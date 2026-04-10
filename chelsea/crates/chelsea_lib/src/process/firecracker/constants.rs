use std::path::PathBuf;

use uuid::Uuid;

use crate::process::firecracker::config::PathBufJailer;

/// Returns the default log path for a given VM ID: a jailed /firecracker.log
pub fn default_log_path(vm_id: Uuid) -> PathBufJailer {
    PathBufJailer::new(vm_id, PathBuf::from("/firecracker.log"))
}
