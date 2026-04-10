use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct WireGuardPeerDto {
    pub public_key: String,
    pub preshared_key: Option<String>,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    pub last_handshake_epoch_secs: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub persistent_keepalive_interval: Option<u16>,
    pub protocol_version: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct WireGuardInterfaceDto {
    pub interface: String,
    pub listen_port: u16,
    pub private_key: Option<String>,
    pub peers: Vec<WireGuardPeerDto>,
}

#[derive(Debug, Deserialize, Clone, ToSchema)]
pub struct AddWireGuardPeerRequest {
    pub public_key: String,
    #[serde(default)]
    pub preshared_key: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub persistent_keepalive_interval: Option<u16>,
}
