use std::path::PathBuf;

use uuid::Uuid;

use crate::process::firecracker::config::PathBufJailer;

/// A struct representing a state/memfile pair for Firecracker
#[derive(Debug)]
pub struct FirecrackerSnapshotPaths {
    pub mem_file_path: PathBufJailer,
    pub state_file_path: PathBufJailer,
}

impl FirecrackerSnapshotPaths {
    /// Returns the paths for a VM's snapshots, inside the VM's own data dir. Eg: When creating snapshot_id 456 inside
    /// VM 123's jail, new("123", "456") would return something like /srv/jailer/firecracker/123/root/456.[memory|state]
    pub fn new(vm_id: &Uuid, snapshot_id: &Uuid) -> Self {
        Self {
            mem_file_path: PathBufJailer::new(
                vm_id.clone(),
                PathBuf::from(format!("/{snapshot_id}.memory")),
            ),
            state_file_path: PathBufJailer::new(
                vm_id.clone(),
                PathBuf::from(format!("/{snapshot_id}.state")),
            ),
        }
    }
}
