use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
};

use defguard_wireguard_rs::{
    InterfaceConfiguration, WGApi, WireguardInterfaceApi, host::Peer, key::Key, net::IpAddrMask,
};
use serde::Serialize;
use std::time::UNIX_EPOCH;

type Backend = defguard_wireguard_rs::Kernel;

type InnerWGApi = WGApi<Backend>;

const PROXY_PRV_IP: &'static str = "fd00:fe11:deed:0::0";
const ORCHESTRATOR_PRV_IP: &'static str = "fd00:fe11:deed:0::ffff";
const KEEPALIVE: u16 = 45;

#[derive(Debug, Serialize)]
pub struct WireGuardPeerInfo {
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

pub struct WG(InnerWGApi);

impl WG {
    pub fn new(
        proxy_prv_key: String,
        orch_pub_key: String,
        orch_pub_ip: String,
        proxy_wg_port: u16,
        orch_wg_port: u16,
    ) -> Self {
        const IFNAME: &str = "wgproxy";

        tracing::info!(interface = IFNAME, "Initializing WireGuard interface");

        tracing::debug!(
            interface = IFNAME,
            proxy_wg_port = %proxy_wg_port,
            orch_wg_port = %orch_wg_port,
            "WireGuard configuration"
        );

        tracing::debug!(interface = IFNAME, "Creating WireGuard interface");
        let wgapi = WGApi::<defguard_wireguard_rs::Kernel>::new(IFNAME.to_string()).unwrap();
        wgapi.create_interface().unwrap();
        tracing::debug!(interface = IFNAME, "WireGuard interface created");

        tracing::debug!(
            orch_pub_ip = %orch_pub_ip,
            orch_wg_port = %orch_wg_port,
            "Configuring orchestrator peer"
        );

        let orch_key = Key::from_str(orch_pub_key.as_str()).unwrap();
        let mut orchestrator = Peer::new(orch_key.clone());
        let endpoint = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::from_str(orch_pub_ip.as_str()).unwrap()),
            orch_wg_port,
        );
        let addr = IpAddrMask::new(
            IpAddr::V6(Ipv6Addr::from_str(ORCHESTRATOR_PRV_IP).unwrap()),
            64,
        );
        orchestrator.endpoint = Some(endpoint);
        orchestrator.persistent_keepalive_interval = Some(KEEPALIVE);
        orchestrator.allowed_ips.push(addr);

        tracing::debug!(
            endpoint = %endpoint,
            keepalive = KEEPALIVE,
            allowed_ips = ?orchestrator.allowed_ips,
            "Orchestrator peer configured"
        );

        let proxy_prv_ip =
            IpAddrMask::new(IpAddr::V6(Ipv6Addr::from_str(PROXY_PRV_IP).unwrap()), 64);
        let interface_config = InterfaceConfiguration {
            name: IFNAME.to_string(),
            prvkey: proxy_prv_key,
            addresses: vec![proxy_prv_ip],
            // A bug in defguard_wireguard_rs. Ports can't actually be 32 bits.
            port: proxy_wg_port as u32,
            peers: vec![orchestrator],
            mtu: None,
        };

        tracing::debug!(
            interface = IFNAME,
            proxy_ip = %PROXY_PRV_IP,
            proxy_wg_port = %proxy_wg_port,
            "Configuring WireGuard interface"
        );

        wgapi.configure_interface(&interface_config).unwrap();
        tracing::debug!(interface = IFNAME, "Interface configured");

        tracing::debug!("Configuring peer routing");
        wgapi
            .configure_peer_routing(&interface_config.peers)
            .unwrap();
        tracing::info!(interface = IFNAME, "WireGuard interface fully initialized");

        Self(wgapi)
    }

    pub fn ensure(
        &self,
        vm_ip: IpAddr,
        node_ip: IpAddr,
        pub_key: String,
        peer_wg_port: u16,
    ) -> anyhow::Result<()> {
        tracing::debug!(ip = %node_ip, pubkey = %pub_key, "Ensuring WireGuard peer");
        let peer_pub_key = match Key::from_str(&pub_key) {
            Ok(k) => k,
            Err(e) => {
                tracing::error!(pubkey = %pub_key, error = %e, "Invalid WireGuard public key");
                return Err(e.into());
            }
        };
        let mut peer = Peer::new(peer_pub_key);
        // todo how this works will have to be tweaked
        peer.set_endpoint(&format!("{node_ip}:{peer_wg_port}"))?;
        match vm_ip {
            IpAddr::V6(ip) => {
                tracing::debug!(
                    vm_ip = %ip,
                    "Setting IPv6 allowed IPs for WireGuard peer"
                );
                peer.set_allowed_ips(vec![IpAddrMask::new(IpAddr::V6(ip), 128)]);
            }
            IpAddr::V4(ip) => {
                tracing::warn!(
                    vm_ip = %ip,
                    "Got an IPv4 address for a VM (expected IPv6), trying anyway"
                );
                peer.set_allowed_ips(vec![IpAddrMask::new(IpAddr::V4(ip), 32)]);
            }
        }
        match self.0.configure_peer(&peer) {
            Ok(_) => {
                tracing::info!(ip = %node_ip, "Successfully configured WireGuard peer");

                // Configure routing for the peer
                if let Err(e) = self.0.configure_peer_routing(&[peer]) {
                    tracing::error!(ip = %node_ip, error = %e, "Failed to configure peer routing");
                    return Err(e.into());
                }

                tracing::info!(ip = %node_ip, "Successfully configured peer routing");
                Ok(())
            }
            Err(e) => {
                tracing::error!(ip = %node_ip, error = %e, "Failed to configure WireGuard peer");
                Err(e.into())
            }
        }
    }

    pub fn remove_peer(&self, public_key: &str) -> anyhow::Result<()> {
        tracing::debug!(pubkey = %public_key, "Remove WireGuard peer");
        let peer_pub_key = match Key::from_str(&public_key) {
            Ok(k) => k,
            Err(e) => {
                tracing::error!(pubkey = %public_key, error = %e, "Invalid WireGuard public key");
                return Err(e.into());
            }
        };

        match self.0.remove_peer(&peer_pub_key) {
            Ok(_) => {
                tracing::info!(peer_pub_key = %peer_pub_key, "Successfully removed WireGuard peer");
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to configure WireGuard peer");
                Err(e.into())
            }
        }
    }
    pub fn list_peers(&self) -> anyhow::Result<Vec<WireGuardPeerInfo>> {
        let host = self.0.read_interface_data()?;
        let peers = host
            .peers
            .values()
            .map(|peer| WireGuardPeerInfo {
                public_key: peer.public_key.to_string(),
                preshared_key: peer.preshared_key.as_ref().map(|key| key.to_string()),
                endpoint: peer.endpoint.as_ref().map(|addr| addr.to_string()),
                allowed_ips: peer.allowed_ips.iter().map(|ip| ip.to_string()).collect(),
                last_handshake_epoch_secs: peer.last_handshake.and_then(|handshake| {
                    handshake
                        .duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|duration| duration.as_secs())
                }),
                rx_bytes: peer.rx_bytes,
                tx_bytes: peer.tx_bytes,
                persistent_keepalive_interval: peer.persistent_keepalive_interval,
                protocol_version: peer.protocol_version,
            })
            .collect();
        Ok(peers)
    }
}
