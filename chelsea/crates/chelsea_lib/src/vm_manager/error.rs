use std::{fmt::Display, net::Ipv4Addr};

use thiserror::Error;
use uuid::Uuid;

use crate::{
    network_manager::error::ReserveNetworkError, process::VmProcessError, store_error::StoreError,
};

#[derive(Error, Debug)]
pub enum VmManagerError {
    #[error("unknown vm manager error: {0:#}")]
    Other(#[from] anyhow::Error),
    #[error("block device resized but guest filesystem resize failed: {0}")]
    Resize2fsFailed(#[source] anyhow::Error),
    #[error("chelsea_db store error: {0}")]
    Store(#[from] StoreError),
    #[error("allocation error: {0}")]
    Allocation(#[from] VmAllocationError),
    #[error("vm lifecycle error: {0}")]
    VmProcess(#[from] VmProcessError),
    #[error("vm lifecycle error: {0}")]
    VmLifecycle(#[from] VmLifecycleError),
    #[error("vm lifecycle error: {0}")]
    ReservedNetworkError(#[from] ReserveNetworkError),
    #[error("error creating VM volume from base image: {0}")]
    CreateVmVolumeFromImageError(
        #[from] crate::volume_manager::error::CreateVmVolumeFromImageError,
    ),
    #[error("openssh error: {0}")]
    OpenSsh(#[from] ssh_key::Error),
    #[error("vm boot/ready error: {0}")]
    ReadyService(#[from] crate::ready_service::error::VmReadyServiceError),
    #[error("vm boot error: {0}")]
    VmBoot(#[from] crate::ready_service::error::VmBootError),
    #[error("error receiving from broadcast channel: {0}")]
    BroadcastRecv(#[from] tokio::sync::broadcast::error::RecvError),
    #[error("db error: {0}")]
    Database(#[from] vers_pg::Error),
    #[error("system time error: {0}")]
    SystemTimeError(#[from] std::time::SystemTimeError),
    #[error("vsock error: {0}")]
    Vsock(#[from] crate::vsock::VsockError),
}

#[derive(Debug, Error)]
pub enum VmLookupError {
    #[error("VM '{vm_id}' not found")]
    Vm { vm_id: String },
    #[error("VM network with host address '{vm_network_host_addr}' not found")]
    Network { vm_network_host_addr: Ipv4Addr },
}

#[derive(Debug, Error)]
pub enum VmLifecycleError {
    #[error("VM {vm_id} is not finished booting. Try again shortly.")]
    StillBooting { vm_id: String },
    #[error("cannot perform operation: VM {vm_id} is paused")]
    IsPaused { vm_id: String },
    #[error("cannot perform operation: VM {vm_id} is not sleeping")]
    IsNotSleeping { vm_id: Uuid },
}

#[derive(Debug, Error)]
pub enum VmAllocationError {
    #[error("Host has reached or exceeded maximum VM count: {current} of {max}")]
    VmCountExceeded { current: u64, max: u64 },
    #[error("Requested VM violates hard maximum for {ty}. Requested: {requested}; Maximum: {max}")]
    HardMaximumViolation {
        ty: VmAllocationType,
        requested: u32,
        max: u32,
    },
    #[error(
        "Host does not have adequate {ty} for requested VM. Requested: {requested}; Available: {available}"
    )]
    InsufficientResources {
        ty: VmAllocationType,
        requested: u32,
        available: u32,
    },
}

#[derive(Debug)]
pub enum VmAllocationType {
    Vcpu,
    Memory,
    Volume,
}

impl Display for VmAllocationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vcpu => write!(f, "vCPU count"),
            Self::Memory => write!(f, "memory MiB"),
            Self::Volume => write!(f, "volume MiB"),
        }
    }
}
