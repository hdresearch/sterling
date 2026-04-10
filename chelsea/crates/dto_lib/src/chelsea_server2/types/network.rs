use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct VmNetworkInfoDto {
    pub vm_id: String,
    pub host_ipv4: String,
    pub vm_ipv4: String,
    pub netns_name: String,
    pub ssh_port: u16,
    pub wireguard: Option<VmNetworkWireGuardInfoDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct VmNetworkWireGuardInfoDto {
    pub interface_name: String,
    pub private_key: String,
    pub private_ipv6: String,
    pub peer_public_key: String,
    pub peer_public_ip: String,
    pub peer_ipv6: String,
}
