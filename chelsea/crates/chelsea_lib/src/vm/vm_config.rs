use std::net::{Ipv4Addr, Ipv6Addr};

use ssh_key::{private::Ed25519Keypair, rand_core::OsRng};
use vers_config::HypervisorType;

/// WireGuard configuration for a VM
#[derive(Debug, Clone)]
pub struct VmWireGuardConfig {
    pub interface_name: String,
    pub private_ip: Ipv6Addr,
    pub peer_pub_key: String,
    pub private_key: String,

    pub peer_ipv6: Ipv6Addr,
    pub peer_pub_ip: Ipv4Addr,
    pub wg_port: u16,
}

/// Configuration parameters common to all VMs
#[derive(Debug)]
pub struct VmConfig {
    pub kernel_name: String,
    pub base_image: String,
    pub vcpu_count: u32,
    pub mem_size_mib: u32,
    pub fs_size_mib: u32,
    pub ssh_keypair: Ed25519Keypair,
    pub hypervisor_type: HypervisorType,
    // pub wireguard: VmWireGuardConfig,
}

impl VmConfig {
    /// Creates a clone of the current config, allowing a new image name to be used, and generating a new keypair
    pub fn new_child(&self, base_image: String /*, wireguard: VmWireGuardConfig*/) -> Self {
        Self {
            kernel_name: self.kernel_name.clone(),
            base_image,
            vcpu_count: self.vcpu_count,
            mem_size_mib: self.mem_size_mib,
            fs_size_mib: self.fs_size_mib,
            ssh_keypair: Ed25519Keypair::random(&mut OsRng),
            hypervisor_type: self.hypervisor_type,
            // wireguard,
        }
    }
}
