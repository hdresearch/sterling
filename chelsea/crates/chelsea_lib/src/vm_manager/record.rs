use std::net::Ipv4Addr;

use uuid::Uuid;

/// Represents a VM in persistent storage. Has a possible foreign key constraint on network_host_addr and process_pid
pub struct VmRecord {
    pub id: Uuid,
    /// SSH key stringified in OpenSSL format; LF line ending
    pub ssh_public_key: String,
    /// SSH key stringified in OpenSSL format; LF line ending
    pub ssh_private_key: String,
    pub kernel_name: String,
    pub image_name: String,
    pub vcpu_count: u32,
    pub mem_size_mib: u32,
    pub fs_size_mib: u32,
    /// Possible foreign key constraint against the VmNetwork table's primary key
    pub vm_network_host_addr: Ipv4Addr,
    /// Possible foreign key constraint against the VmProcess table's primary key
    pub vm_process_pid: u32,
    /// Possible foreign key constraint against the VmVolume table's primary key
    pub vm_volume_id: Uuid,
}
