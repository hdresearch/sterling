use thiserror::Error;
use uuid::Uuid;

use crate::process::cloud_hypervisor::{CloudHypervisorProcess, CloudHypervisorProcessError};
use crate::process::firecracker::{FirecrackerProcess, FirecrackerProcessError};

pub use vers_config::HypervisorType;

/// Represents a VM process handle
#[derive(Debug)]
pub enum VmProcess {
    Firecracker(FirecrackerProcess),
    CloudHypervisor(CloudHypervisorProcess),
}

#[derive(Debug, Error)]
pub enum VmProcessError {
    #[error("firecracker error: {0}")]
    Firecracker(#[from] FirecrackerProcessError),
    #[error("cloud-hypervisor error: {0}")]
    CloudHypervisor(#[from] CloudHypervisorProcessError),
}

impl From<FirecrackerProcess> for VmProcess {
    fn from(value: FirecrackerProcess) -> Self {
        Self::Firecracker(value)
    }
}

impl From<CloudHypervisorProcess> for VmProcess {
    fn from(value: CloudHypervisorProcess) -> Self {
        Self::CloudHypervisor(value)
    }
}

impl VmProcess {
    pub async fn pid(&self) -> Result<u32, VmProcessError> {
        match self {
            VmProcess::Firecracker(process) => Ok(process.pid().await?),
            VmProcess::CloudHypervisor(process) => Ok(process.pid().await?),
        }
    }

    pub async fn kill(&self) -> Result<(), VmProcessError> {
        match self {
            VmProcess::Firecracker(process) => Ok(process.kill().await?),
            VmProcess::CloudHypervisor(process) => Ok(process.kill().await?),
        }
    }

    pub fn process_type(&self) -> HypervisorType {
        match self {
            VmProcess::Firecracker(_) => HypervisorType::Firecracker,
            VmProcess::CloudHypervisor(_) => HypervisorType::CloudHypervisor,
        }
    }

    pub fn vm_id(&self) -> Uuid {
        match self {
            VmProcess::Firecracker(process) => process.vm_id(),
            VmProcess::CloudHypervisor(process) => process.vm_id(),
        }
    }

    pub async fn is_paused(&self) -> anyhow::Result<bool> {
        match self {
            VmProcess::Firecracker(process) => process.is_paused().await,
            VmProcess::CloudHypervisor(process) => process.is_paused().await,
        }
    }

    pub async fn pause(&self) -> anyhow::Result<()> {
        match self {
            VmProcess::Firecracker(process) => process.pause().await,
            VmProcess::CloudHypervisor(process) => process.pause().await,
        }
    }

    pub async fn resume(&self) -> anyhow::Result<()> {
        match self {
            VmProcess::Firecracker(process) => process.resume().await,
            VmProcess::CloudHypervisor(process) => process.resume().await,
        }
    }

    /// Notify the hypervisor that a block device's backing store has changed size.
    /// path is jail-relative.
    pub async fn update_drive(
        &self,
        drive_id: &str,
        jail_relative_path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<()> {
        match self {
            VmProcess::Firecracker(process) => {
                process.update_drive(drive_id, jail_relative_path).await
            }
            VmProcess::CloudHypervisor(process) => {
                process.update_drive(drive_id, jail_relative_path).await
            }
        }
    }
}
