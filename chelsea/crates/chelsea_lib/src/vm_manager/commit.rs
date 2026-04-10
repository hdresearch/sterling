use serde::{Deserialize, Serialize};
use ssh_key::PrivateKey;
use utoipa::ToSchema;
use uuid::Uuid;
use vers_pg::schema::chelsea::tables::commit::{CommitFile, RecordCommit};

use crate::{
    process_manager::VmProcessCommitMetadata, vm::VmConfig, volume_manager::VmVolumeCommitMetadata,
};

/// Contains information about the VM to be stored as a JSON object in the VmCommitStore
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VmCommitMetadata {
    pub commit_id: Uuid,
    /// The CPU architecture of the original host machine
    pub host_architecture: String,
    pub process_metadata: VmProcessCommitMetadata,
    pub volume_metadata: VmVolumeCommitMetadata,
    pub vm_config: VmConfigCommit,
    /// The file names on the VmCommitStore that were uploaded/need to be downloaded for this commit.
    pub remote_files: Vec<CommitFile>,
}

impl Into<RecordCommit> for VmCommitMetadata {
    fn into(self) -> RecordCommit {
        RecordCommit {
            id: self.commit_id,
            host_architecture: self.host_architecture,
            kernel_name: self.vm_config.kernel_name,
            base_image: self.vm_config.base_image,
            vcpu_count: self.vm_config.vcpu_count,
            mem_size_mib: self.vm_config.mem_size_mib,
            fs_size_mib: self.vm_config.fs_size_mib,
            ssh_public_key: self.vm_config.ssh_public_key,
            ssh_private_key: self.vm_config.ssh_private_key,
            process_commit: self.process_metadata.into(),
            volume_commit: self.volume_metadata.into(),
            remote_files: self.remote_files,
            deleted_at: None,
            deleted_by: None,
        }
    }
}

/// Represents a VmConfig in a serializable format
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct VmConfigCommit {
    pub kernel_name: String,
    pub base_image: String,
    pub vcpu_count: u32,
    pub mem_size_mib: u32,
    pub fs_size_mib: u32,
    pub ssh_public_key: String,
    pub ssh_private_key: String,
}

impl TryFrom<VmConfig> for VmConfigCommit {
    type Error = anyhow::Error;
    fn try_from(value: VmConfig) -> Result<Self, Self::Error> {
        let private_key = PrivateKey::from(value.ssh_keypair);
        let ssh_public_key = private_key.public_key().to_openssh()?;
        let ssh_private_key = private_key.to_openssh(ssh_key::LineEnding::LF)?.to_string();

        Ok(Self {
            kernel_name: value.kernel_name,
            base_image: value.base_image,
            vcpu_count: value.vcpu_count,
            mem_size_mib: value.mem_size_mib,
            fs_size_mib: value.fs_size_mib,
            ssh_public_key,
            ssh_private_key,
        })
    }
}
