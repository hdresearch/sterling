use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use vers_config::VersConfig;
use vers_pg::schema::chelsea::tables::commit::{
    RecordCloudHypervisorProcessCommit, RecordFirecrackerProcessCommit, RecordProcessCommit,
};

/// Contains information about the process to be stored on the VmCommitMetadata
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub enum VmProcessCommitMetadata {
    Firecracker(VmFirecrackerProcessCommitMetadata),
    CloudHypervisor(VmCloudHypervisorProcessCommitMetadata),
}

impl From<RecordProcessCommit> for VmProcessCommitMetadata {
    fn from(value: RecordProcessCommit) -> Self {
        match value {
            RecordProcessCommit::Firecracker(record) => {
                Self::Firecracker(VmFirecrackerProcessCommitMetadata::from(record))
            }
            RecordProcessCommit::CloudHypervisor(record) => {
                Self::CloudHypervisor(VmCloudHypervisorProcessCommitMetadata::from(record))
            }
        }
    }
}

impl Into<RecordProcessCommit> for VmProcessCommitMetadata {
    fn into(self) -> RecordProcessCommit {
        match self {
            VmProcessCommitMetadata::Firecracker(metadata) => {
                RecordProcessCommit::Firecracker(metadata.into())
            }
            VmProcessCommitMetadata::CloudHypervisor(metadata) => {
                RecordProcessCommit::CloudHypervisor(metadata.into())
            }
        }
    }
}

/// Metadata associated with a Firecracker process on commit
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct VmFirecrackerProcessCommitMetadata {}

impl From<RecordFirecrackerProcessCommit> for VmFirecrackerProcessCommitMetadata {
    fn from(_: RecordFirecrackerProcessCommit) -> Self {
        VmFirecrackerProcessCommitMetadata {}
    }
}

impl Into<RecordFirecrackerProcessCommit> for VmFirecrackerProcessCommitMetadata {
    fn into(self) -> RecordFirecrackerProcessCommit {
        RecordFirecrackerProcessCommit {}
    }
}

/// Metadata associated with a CloudHypervisor process on commit
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct VmCloudHypervisorProcessCommitMetadata {}

impl From<RecordCloudHypervisorProcessCommit> for VmCloudHypervisorProcessCommitMetadata {
    fn from(_: RecordCloudHypervisorProcessCommit) -> Self {
        VmCloudHypervisorProcessCommitMetadata {}
    }
}

impl Into<RecordCloudHypervisorProcessCommit> for VmCloudHypervisorProcessCommitMetadata {
    fn into(self) -> RecordCloudHypervisorProcessCommit {
        RecordCloudHypervisorProcessCommit {}
    }
}

/// Represents the expected location for Firecracker mem and snapshot files within the commits data directory
pub struct VmFirecrackerProcessCommitFilepaths {
    pub mem_file_path: PathBuf,
    pub state_file_path: PathBuf,
}

impl VmFirecrackerProcessCommitFilepaths {
    /// Returns the Firecracker mem and snapshot filepaths for a given commit ID
    pub async fn from_commit_id(commit_id: &Uuid) -> anyhow::Result<Self> {
        let snapshot_dir = &VersConfig::chelsea().snapshot_dir;

        let mem_file_path = snapshot_dir.join(format!("{commit_id}.memory"));
        let state_file_path = snapshot_dir.join(format!("{commit_id}.state"));

        Ok(Self {
            mem_file_path,
            state_file_path,
        })
    }
}

/// Represents the expected location for CloudHypervisor snapshot tar file within the commits data directory
pub struct VmCloudHypervisorProcessCommitFilepaths {
    pub snapshot_tar_path: PathBuf,
}

impl VmCloudHypervisorProcessCommitFilepaths {
    /// Returns the CloudHypervisor snapshot tar filepath for a given commit ID
    pub async fn from_commit_id(commit_id: &str) -> anyhow::Result<Self> {
        let snapshot_dir = &VersConfig::chelsea().snapshot_dir;
        let snapshot_tar_path = snapshot_dir.join(format!("{}.ch_snapshot.tar", commit_id));

        Ok(Self { snapshot_tar_path })
    }
}
