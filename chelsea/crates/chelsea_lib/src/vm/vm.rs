use std::sync::Arc;

use ssh_key::PrivateKey;
use uuid::Uuid;

use crate::{
    network::VmNetwork, process::VmProcess, vm::VmConfig, vm_manager::VmRecord, volume::VmVolume,
};

/// Represents a VM, with handles for its process and volume
pub struct Vm {
    pub id: Uuid,
    pub config: VmConfig,
    pub process: Arc<VmProcess>,
    pub network: VmNetwork,
    pub volume: Arc<dyn VmVolume>,
}

impl Vm {
    pub fn new(
        id: Uuid,
        config: VmConfig,
        process: Arc<VmProcess>,
        network: VmNetwork,
        volume: Arc<dyn VmVolume>,
    ) -> Self {
        Self {
            id,
            config,
            process,
            network,
            volume,
        }
    }

    /// Represent the VM as a record to be inserted in to the VmStore
    pub async fn as_record(&self) -> anyhow::Result<VmRecord> {
        Ok(VmRecord {
            id: self.id.clone(),
            ssh_public_key: self.config.ssh_keypair.public.to_string(),
            ssh_private_key: PrivateKey::from(self.config.ssh_keypair.clone())
                .to_openssh(ssh_key::LineEnding::LF)?
                .to_string(),
            kernel_name: self.config.kernel_name.clone(),
            image_name: self.config.base_image.clone(),
            vcpu_count: self.config.vcpu_count,
            mem_size_mib: self.config.mem_size_mib,
            fs_size_mib: self.config.fs_size_mib,
            vm_network_host_addr: self.network.host_addr.clone(),
            vm_process_pid: self.process.pid().await?,
            vm_volume_id: self.volume.id(),
        })
    }
}
