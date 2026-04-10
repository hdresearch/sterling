use std::path::PathBuf;

use vers_config::VersConfig;

/// Represents the expected location for Firecracker mem and snapshot files within the snapshots data directory
pub struct VmFirecrackerProcessSleepSnapshotFilepaths {
    pub mem_file_path: PathBuf,
    pub state_file_path: PathBuf,
}

impl VmFirecrackerProcessSleepSnapshotFilepaths {
    /// Returns the Firecracker mem and snapshot filepaths for a given snapshot ID
    pub async fn from_snapshot_id(snapshot_id: &str) -> anyhow::Result<Self> {
        let snapshot_dir = &VersConfig::chelsea().snapshot_dir;

        let mem_file_path = snapshot_dir.join(format!("{snapshot_id}.memory"));
        let state_file_path = snapshot_dir.join(format!("{snapshot_id}.state"));

        Ok(Self {
            mem_file_path,
            state_file_path,
        })
    }
}

/// Represents the expected location for CloudHypervisor snapshot tar file within the snapshots data directory
pub struct VmCloudHypervisorProcessSleepSnapshotFilepaths {
    pub snapshot_tar_path: PathBuf,
}

impl VmCloudHypervisorProcessSleepSnapshotFilepaths {
    /// Returns the CloudHypervisor snapshot tar filepath for a given VM ID
    pub async fn from_vm_id(vm_id: &str) -> anyhow::Result<Self> {
        let snapshot_dir = &VersConfig::chelsea().snapshot_dir;
        let snapshot_tar_path = snapshot_dir.join(format!("{}.ch_snapshot.tar", vm_id));

        Ok(Self { snapshot_tar_path })
    }
}
