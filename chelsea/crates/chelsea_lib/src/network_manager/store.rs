use std::net::Ipv4Addr;

use async_trait::async_trait;
use chrono::Utc;

use crate::{network::VmNetwork, store_error::StoreError, vm::VmWireGuardConfig};

#[async_trait]
pub trait VmNetworkManagerStore: Send + Sync {
    async fn insert_vm_network(&self, vm_network: VmNetworkRecord) -> Result<(), StoreError>;
    async fn set_wg_on_vm_network(
        &self,
        host_addr: &Ipv4Addr,
        vm_network: Option<VmWireGuardConfig>,
    ) -> Result<Option<()>, StoreError>;
    async fn fetch_vm_network(&self, host_addr: &Ipv4Addr)
    -> Result<Option<VmNetwork>, StoreError>;
    async fn check_vm_network_exists(&self, host_addr: &Ipv4Addr) -> Result<bool, StoreError>;
    async fn delete_vm_network(&self, host_addr: &Ipv4Addr) -> Result<(), StoreError>;
    async fn reserve_network(&self) -> Result<Option<VmNetwork>, StoreError>;
    async fn unreserve_network(&self, host_addr: &Ipv4Addr) -> Result<(), StoreError>;
}

/// Represents a VmNetwork in the store
pub struct VmNetworkRecord {
    pub host_addr: u32,
    pub vm_addr: u32,
    pub netns_name: String,
    pub ssh_port: u16,
    pub wg: Option<VmWireGuardConfig>,
    /// RFC 3339; On reserving a network, a VM will be given VM_NETWORK_RESERVE_TIMEOUT_SECS seconds to be inserted into the database, or its VmNetwork will be conisdered free.
    pub reserved_until: String,
}

impl From<&VmNetwork> for VmNetworkRecord {
    fn from(value: &VmNetwork) -> Self {
        Self {
            host_addr: value.host_addr.to_bits(),
            vm_addr: value.vm_addr.to_bits(),
            netns_name: value.netns_name.clone(),
            ssh_port: value.ssh_port,
            wg: value.wg.clone(),
            reserved_until: Utc::now().to_rfc3339(),
        }
    }
}
